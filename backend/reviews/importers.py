"""Import von RIS und BibTeX in ein einheitliches Format."""
import re

FIELDS = ("title", "abstract", "authors", "year", "journal", "doi", "pmid", "url", "keywords")


def detect_format(filename, text):
    name = (filename or "").lower()
    if name.endswith((".bib", ".bibtex")):
        return "bibtex"
    if name.endswith((".ris", ".txt", ".nbib")):
        return "ris"
    head = text.lstrip()[:200]
    if head.startswith("@"):
        return "bibtex"
    if re.search(r"^TY  - ", text, flags=re.M):
        return "ris"
    raise ValueError("Format nicht erkannt. Unterstützt werden RIS und BibTeX.")


def _first(entry, *keys):
    for key in keys:
        value = entry.get(key)
        if value:
            return value
    return ""


def _text(value):
    if isinstance(value, (list, tuple)):
        value = "; ".join(str(v) for v in value if v)
    return re.sub(r"\s+", " ", str(value or "")).strip()


def _year(value):
    match = re.search(r"\b(1[5-9]\d\d|20\d\d)\b", _text(value))
    return int(match.group(1)) if match else None


def _doi(value):
    doi = _text(value).lower()
    doi = re.sub(r"^(https?://(dx\.)?doi\.org/|doi:\s*)", "", doi)
    return doi[:255]


def _jsonable(value):
    if isinstance(value, dict):
        return {str(k): _jsonable(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [_jsonable(v) for v in value]
    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    return str(value)


def parse_ris(text):
    import rispy

    out = []
    for entry in rispy.loads(text):
        out.append({
            "title": _text(_first(entry, "title", "primary_title", "translated_title", "secondary_title")),
            "abstract": _text(_first(entry, "abstract", "notes_abstract")),
            "authors": _text(_first(entry, "authors", "first_authors")),
            "year": _year(_first(entry, "year", "publication_year", "date")),
            "journal": _text(_first(entry, "journal_name", "alternate_title3", "alternate_title2", "secondary_title")),
            "doi": _doi(_first(entry, "doi")),
            "pmid": "",
            "url": _text(_first(entry, "urls", "url")),
            "keywords": _text(_first(entry, "keywords")),
            "raw": _jsonable(entry),
        })
    return out


def parse_bibtex(text):
    import bibtexparser

    def clean(value):
        return _text(str(value or "").replace("{", "").replace("}", ""))

    out = []
    for entry in bibtexparser.loads(text).entries:
        out.append({
            "title": clean(entry.get("title")),
            "abstract": clean(entry.get("abstract")),
            "authors": clean(entry.get("author")).replace(" and ", "; "),
            "year": _year(entry.get("year") or entry.get("date")),
            "journal": clean(_first(entry, "journal", "journaltitle", "booktitle")),
            "doi": _doi(entry.get("doi")),
            "pmid": clean(entry.get("pmid"))[:32],
            "url": clean(entry.get("url")),
            "keywords": clean(entry.get("keywords")),
            "raw": _jsonable(entry),
        })
    return out


def parse(fmt, text):
    if fmt == "ris":
        return parse_ris(text)
    if fmt == "bibtex":
        return parse_bibtex(text)
    raise ValueError(f"Unbekanntes Format: {fmt}")


def dedup_key(item):
    """Schlüssel für exakte Dubletten: DOI, sonst normalisierter Titel plus Jahr. Leer = keine Prüfung."""
    if item.get("doi"):
        return f"doi:{item['doi']}"[:300]
    title = re.sub(r"[^a-z0-9]+", "", (item.get("title") or "").lower())
    if len(title) < 10:
        return ""
    return f"ty:{title}|{item.get('year') or ''}"[:300]
