from reviews import importers

RIS = """TY  - JOUR
AU  - Miller, Anna
AU  - Chen, Li
TI  - Deep learning for headache classification
AB  - We evaluate neural networks.
PY  - 2021/03/01
JO  - Journal of Tests
DO  - https://doi.org/10.1000/ABC123
KW  - headache
KW  - deep learning
UR  - https://example.org/a
ER  -
"""

BIBTEX = """@article{smith2020,
  title = {A {Study} of things},
  author = {Smith, John and Doe, Jane},
  year = {2020},
  journal = {Journal of Things},
  doi = {10.1000/XYZ}
}
"""


def test_ris():
    (item,) = importers.parse("ris", RIS)
    assert item["title"] == "Deep learning for headache classification"
    assert item["abstract"] == "We evaluate neural networks."
    assert item["authors"] == "Miller, Anna; Chen, Li"
    assert item["year"] == 2021
    assert item["journal"] == "Journal of Tests"
    assert item["doi"] == "10.1000/abc123"
    assert item["keywords"] == "headache; deep learning"
    assert importers.dedup_key(item) == "doi:10.1000/abc123"


def test_bibtex():
    (item,) = importers.parse("bibtex", BIBTEX)
    assert item["title"] == "A Study of things"
    assert item["authors"] == "Smith, John; Doe, Jane"
    assert item["year"] == 2020
    assert item["journal"] == "Journal of Things"
    assert item["doi"] == "10.1000/xyz"


def test_dedup_key_without_doi():
    a = {"doi": "", "title": "Rule-based triage of Migraine patients", "year": 2019}
    b = {"doi": "", "title": "Rule-Based Triage of migraine patients.", "year": 2019}
    assert importers.dedup_key(a) == importers.dedup_key(b) != ""
    assert importers.dedup_key({"doi": "", "title": "Kurz", "year": 2019}) == ""


def test_detect_format():
    assert importers.detect_format("a.bib", "") == "bibtex"
    assert importers.detect_format("a.ris", "") == "ris"
    assert importers.detect_format("export", RIS) == "ris"
    assert importers.detect_format("export", BIBTEX) == "bibtex"
