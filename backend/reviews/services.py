"""Fachlogik. Alle Entscheidungen werden als Ereignisse angehängt; gelesen wird aus den SQL-Projektionen."""
import hashlib
import json
import random
import uuid

from django.contrib.auth import get_user_model
from django.db import connection, transaction

from eventcore import store
from eventcore.models import Event, Review

from . import importers
from .models import Actor, ImportBatch, Membership, Record, StageAssignment

MANAGE_ROLES = {"owner", "admin"}
WRITE_ROLES = {"owner", "admin", "reviewer"}
DECISIONS = {"include", "exclude", "maybe"}
STAGE_EVENTS = ("StageInstanceOpened", "StageUnblinded", "StageInstanceClosed")


class DomainError(Exception):
    def __init__(self, message, status=400):
        super().__init__(message)
        self.message = message
        self.status = status


def rows(sql, params=None):
    with connection.cursor() as cur:
        cur.execute(sql, params or [])
        cols = [c[0] for c in cur.description]
        return [dict(zip(cols, r)) for r in cur.fetchall()]


def one(sql, params=None):
    found = rows(sql, params)
    return found[0] if found else {}


def _json(value):
    """Rohabfragen liefern jsonb je nach Treiber als Text."""
    return json.loads(value) if isinstance(value, str) else value


def actor_id(user):
    actor, _ = Actor.objects.get_or_create(user=user)
    return actor.actor_id


def usernames(actor_ids):
    actors = Actor.objects.filter(actor_id__in=list(actor_ids)).select_related("user")
    return {str(a.actor_id): a.user.username for a in actors}


# ---------------------------------------------------------------- Reviews und Mitglieder

@transaction.atomic
def create_review(user, title):
    review = Review.objects.create(title=title)
    Membership.objects.create(review=review, user=user, role="owner")
    actor = actor_id(user)
    store.append(review.review_id, "ReviewCreated", actor, {"title": title})
    store.append(review.review_id, "MemberRoleSet", actor, {"actor_id": str(actor), "role": "owner"})
    return review


@transaction.atomic
def set_member(review_id, by_user, username, role):
    if role not in Membership.ROLES:
        raise DomainError("Unbekannte Rolle.")
    user = get_user_model().objects.filter(username=username).first()
    if user is None:
        raise DomainError("Diesen Benutzer gibt es nicht.", 404)
    current = Membership.objects.filter(review_id=review_id, user=user).first()
    if current and current.role == "owner" and role != "owner":
        if Membership.objects.filter(review_id=review_id, role="owner").count() == 1:
            raise DomainError("Ein Review braucht mindestens einen Owner.")
    Membership.objects.update_or_create(review_id=review_id, user=user, defaults={"role": role})
    store.append(review_id, "MemberRoleSet", actor_id(by_user), {"actor_id": str(actor_id(user)), "role": role})


# ---------------------------------------------------------------- Kriterien

EMPTY_CRITERIA = {"version": 0, "text": "", "include_keywords": [], "exclude_keywords": [], "exclusion_reasons": []}


def current_criteria(review_id):
    event = Event.objects.filter(review_id=review_id, type="CriteriaVersionPublished").order_by("-seq").first()
    return event.payload if event else dict(EMPTY_CRITERIA)


def _str_list(value):
    if isinstance(value, str):
        value = value.replace("\n", ",").split(",")
    return [str(v).strip() for v in (value or []) if str(v).strip()]


def publish_criteria(review_id, user, data):
    previous = current_criteria(review_id)
    payload = {
        "version": previous["version"] + 1,
        "previous_version": previous["version"] or None,
        "text": str(data.get("text", "")),
        "include_keywords": _str_list(data.get("include_keywords")),
        "exclude_keywords": _str_list(data.get("exclude_keywords")),
        "exclusion_reasons": _str_list(data.get("exclusion_reasons")),
        "change_note": str(data.get("change_note", "")),
    }
    store.append(review_id, "CriteriaVersionPublished", actor_id(user), payload)
    return payload


# ---------------------------------------------------------------- Import

def create_import(review_id, user, filename, text, source=""):
    fmt = importers.detect_format(filename, text)
    return ImportBatch.objects.create(
        review_id=review_id, filename=filename[:255], fmt=fmt, source=source[:255], content=text,
        sha256=hashlib.sha256(text.encode("utf-8")).hexdigest(), created_by=user,
    )


def run_import(batch_id):
    batch = ImportBatch.objects.get(pk=batch_id)
    batch.status = "running"
    batch.save(update_fields=["status"])
    try:
        items = importers.parse(batch.fmt, batch.content)
        if not items:
            raise ValueError("Die Datei enthält keine Einträge.")
        with transaction.atomic():
            known = dict(
                Record.objects.filter(review_id=batch.review_id, duplicate_of__isnull=True)
                .values_list("dedup_key", "id")
            )
            new, n_duplicates = [], 0
            for item in items:
                record_id = uuid.uuid4()
                key = importers.dedup_key(item) or f"id:{record_id}"
                original = known.get(key)
                if original is None:
                    known[key] = record_id
                else:
                    n_duplicates += 1
                new.append(Record(
                    id=record_id, review_id=batch.review_id, batch=batch, dedup_key=key, duplicate_of_id=original,
                    title=item["title"], abstract=item["abstract"], authors=item["authors"], year=item["year"],
                    journal=item["journal"], doi=item["doi"], pmid=item["pmid"], url=item["url"],
                    keywords=item["keywords"], raw=item["raw"],
                ))
            Record.objects.bulk_create(new, batch_size=1000)
            ids_hash = hashlib.sha256("\n".join(sorted(str(r.id) for r in new)).encode()).hexdigest()
            store.append(batch.review_id, "RecordsImported", actor_id(batch.created_by), {
                "batch_id": str(batch.id), "source": batch.source, "filename": batch.filename,
                "file_sha256": batch.sha256, "format": batch.fmt, "n_records": len(new),
                "n_duplicates": n_duplicates, "record_ids_sha256": ids_hash,
            })
            batch.status, batch.n_records, batch.n_duplicates, batch.error = "done", len(new), n_duplicates, ""
            batch.save(update_fields=["status", "n_records", "n_duplicates", "error"])
    except Exception as exc:  # Fehler der Datei gehören in den Importstatus, nicht in ein Absturzprotokoll
        batch.status, batch.error = "failed", str(exc)[:2000]
        batch.save(update_fields=["status", "error"])


# ---------------------------------------------------------------- Stufen

def _stage_from_events(events):
    opened = next((e for e in events if e.type == "StageInstanceOpened"), None)
    if opened is None:
        return None
    p = opened.payload
    unblinded = any(e.type == "StageUnblinded" for e in events)
    closed = any(e.type == "StageInstanceClosed" for e in events)
    blind = bool(p.get("blind"))
    return {
        "id": str(opened.stage_instance_id), "review_id": str(opened.review_id),
        "name": p.get("name", ""), "kind": p.get("kind", ""), "blind": blind,
        "min_reviewers": p.get("min_reviewers", 2), "maybe_as": p.get("maybe_as"),
        "criteria_version": p.get("criteria_version"), "scope": p.get("scope"),
        "sample_size": p.get("sample_size"), "seed": p.get("seed"), "source_stage": p.get("source_stage"),
        "n_records": p.get("n_records", 0), "opened_at": opened.occurred_at.isoformat(),
        "unblinded": unblinded, "closed": closed, "blind_active": blind and not unblinded,
    }


def get_stage(stage_id):
    events = list(Event.objects.filter(stage_instance_id=stage_id, type__in=STAGE_EVENTS).order_by("seq"))
    stage = _stage_from_events(events)
    if stage is None:
        raise DomainError("Stufe nicht gefunden.", 404)
    return stage


def list_stages(review_id):
    by_stage = {}
    for event in Event.objects.filter(review_id=review_id, type__in=STAGE_EVENTS).order_by("seq"):
        by_stage.setdefault(event.stage_instance_id, []).append(event)
    return [_stage_from_events(events) for events in by_stage.values()]


def open_stage(review_id, user, *, name, kind="title_abstract", blind=True, min_reviewers=2, maybe_as=None,
               scope="all", sample_size=None, seed=None, source_stage=None):
    if maybe_as not in (None, "include", "exclude"):
        raise DomainError("maybe_as muss leer, include oder exclude sein.")
    if not name:
        raise DomainError("Name der Stufe fehlt.")
    if scope == "from_stage":
        if not source_stage:
            raise DomainError("Quellstufe fehlt.")
        source = get_stage(source_stage)
        if source["review_id"] != str(review_id):
            raise DomainError("Die Quellstufe gehört zu einem anderen Review.")
        ids = sorted(r["record_id"] for r in rows(
            "SELECT record_id FROM final_decision WHERE stage_instance_id = %s::uuid AND decision = 'include'",
            [str(source_stage)]))
    elif scope in ("all", "sample"):
        ids = sorted(str(i) for i in Record.objects.filter(review_id=review_id, duplicate_of__isnull=True)
                     .values_list("id", flat=True))
        if scope == "sample":
            seed = int(seed) if seed is not None else random.SystemRandom().randrange(1, 2**31)
            ids = random.Random(seed).sample(ids, min(int(sample_size or 25), len(ids)))
    else:
        raise DomainError("Unbekannter Umfang der Stufe.")
    if not ids:
        raise DomainError("Für diese Stufe gibt es keine Records.")
    stage_id = uuid.uuid4()
    with transaction.atomic():
        StageAssignment.objects.bulk_create(
            [StageAssignment(stage_instance_id=stage_id, review_id=review_id, record_id=rid, position=i)
             for i, rid in enumerate(ids)], batch_size=2000)
        store.append(review_id, "StageInstanceOpened", actor_id(user), {
            "name": name, "kind": kind, "blind": bool(blind), "min_reviewers": int(min_reviewers),
            "maybe_as": maybe_as, "criteria_version": str(current_criteria(review_id)["version"]),
            "scope": scope, "sample_size": len(ids) if scope == "sample" else None,
            "seed": seed if scope == "sample" else None,
            "source_stage": str(source_stage) if scope == "from_stage" else None, "n_records": len(ids),
        }, stage_id=stage_id)
    return get_stage(stage_id)


def unblind_stage(stage, user):
    if not stage["blind_active"]:
        raise DomainError("Die Stufe ist nicht verblindet.", 409)
    store.append(stage["review_id"], "StageUnblinded", actor_id(user), {}, stage_id=stage["id"])


def close_stage(stage, user):
    if stage["closed"]:
        raise DomainError("Die Stufe ist bereits geschlossen.", 409)
    actor = actor_id(user)
    label = f"Abschluss {stage['name']}"
    with transaction.atomic():
        seq = store.append(stage["review_id"], "StageInstanceClosed", actor, {"label": label}, stage_id=stage["id"])
        snap = rows(
            "INSERT INTO snapshot (review_id, label, up_to_seq, hash) "
            "SELECT review_id, %s, seq, hash FROM event WHERE review_id = %s::uuid AND seq = %s "
            "RETURNING snapshot_id, encode(hash, 'hex') AS hash",
            [label, stage["review_id"], seq])[0]
        store.append(stage["review_id"], "SnapshotTaken", actor, {
            "snapshot_id": str(snap["snapshot_id"]), "label": label, "up_to_seq": seq, "hash": snap["hash"],
        }, stage_id=stage["id"])
    return {"snapshot_id": str(snap["snapshot_id"]), "label": label, "up_to_seq": seq, "hash": snap["hash"]}


# ---------------------------------------------------------------- Abstimmen

def _require_assigned(stage, record_id):
    try:
        record_uuid = uuid.UUID(str(record_id))
    except ValueError:
        raise DomainError("Ungültige Record-ID.")
    if not StageAssignment.objects.filter(stage_instance_id=stage["id"], record_id=record_uuid).exists():
        raise DomainError("Der Record gehört nicht zu dieser Stufe.", 404)
    return str(record_uuid)


def my_votes(stage, user):
    return {r["record_id"]: r["decision"] for r in rows(
        "SELECT record_id, decision FROM current_vote WHERE stage_instance_id = %s::uuid AND actor_id = %s::uuid",
        [stage["id"], str(actor_id(user))])}


def cast_vote(stage, user, record_id, decision, reasons=None, note=""):
    if stage["closed"]:
        raise DomainError("Die Stufe ist geschlossen.", 409)
    if decision not in DECISIONS:
        raise DomainError("Entscheidung muss include, exclude oder maybe sein.")
    record_id = _require_assigned(stage, record_id)
    previous = my_votes(stage, user).get(record_id)
    payload = {"record_id": record_id, "decision": decision, "reasons": _str_list(reasons), "note": str(note or "")}
    if previous is not None:
        payload["previous"] = previous
    store.append(stage["review_id"], "VoteRevised" if previous is not None else "VoteCast",
                 actor_id(user), payload, stage_id=stage["id"])


def flag_undecidable(stage, user, record_id, note=""):
    if stage["closed"]:
        raise DomainError("Die Stufe ist geschlossen.", 409)
    record_id = _require_assigned(stage, record_id)
    store.append(stage["review_id"], "RecordFlaggedUndecidable", actor_id(user),
                 {"record_id": record_id, "note": str(note or "")}, stage_id=stage["id"])


def record_consensus(stage, user, record_id, decision, method="discussion", reasons=None, note=""):
    if stage["closed"]:
        raise DomainError("Die Stufe ist geschlossen.", 409)
    if stage["blind_active"]:
        raise DomainError("Die Stufe ist verblindet. Ein Konsens ist erst nach dem Entblinden möglich.", 403)
    if decision not in ("include", "exclude"):
        raise DomainError("Ein Konsens muss include oder exclude sein.")
    record_id = _require_assigned(stage, record_id)
    store.append(stage["review_id"], "ConsensusRecorded", actor_id(user), {
        "record_id": record_id, "decision": decision, "method": method,
        "reasons": _str_list(reasons), "note": str(note or ""),
    }, stage_id=stage["id"])


def record_dict(record):
    return {
        "id": str(record.id), "title": record.title, "abstract": record.abstract, "authors": record.authors,
        "year": record.year, "journal": record.journal, "doi": record.doi, "url": record.url,
        "keywords": record.keywords, "duplicate_of": str(record.duplicate_of_id) if record.duplicate_of_id else None,
    }


def progress(stage, user):
    return {"done": len(my_votes(stage, user)), "total": stage["n_records"]}


def next_record(stage, user):
    found = rows(
        "SELECT a.record_id FROM reviews_stageassignment a "
        "WHERE a.stage_instance_id = %s::uuid AND NOT EXISTS ("
        "  SELECT 1 FROM current_vote cv WHERE cv.stage_instance_id = a.stage_instance_id "
        "     AND cv.record_id = a.record_id::text AND cv.actor_id = %s::uuid) "
        "ORDER BY a.position LIMIT 1",
        [stage["id"], str(actor_id(user))])
    if not found:
        return None
    return record_dict(Record.objects.get(pk=found[0]["record_id"]))


# ---------------------------------------------------------------- Auswertung

def conflicts(stage):
    """Records mit widersprüchlichen Voten oder Markierung, samt Einzelvoten. Nur nach dem Entblinden aufrufen."""
    open_items = rows(
        "SELECT record_id, status, n_votes FROM final_decision "
        "WHERE stage_instance_id = %s::uuid AND status IN ('disagreement', 'needs_clarification') ORDER BY record_id",
        [stage["id"]])
    ids = [r["record_id"] for r in open_items]
    votes = rows(
        "SELECT cv.record_id, cv.actor_id, cv.decision, e.payload->'reasons' AS reasons, e.payload->>'note' AS note "
        "FROM current_vote cv JOIN event e ON e.review_id = cv.review_id AND e.seq = cv.seq "
        "WHERE cv.stage_instance_id = %s::uuid AND cv.record_id = ANY(%s)", [stage["id"], ids]) if ids else []
    names = usernames({v["actor_id"] for v in votes})
    records = {str(r.id): r for r in Record.objects.filter(pk__in=ids)}
    by_record = {}
    for v in votes:
        by_record.setdefault(v["record_id"], []).append({
            "username": names.get(str(v["actor_id"]), "?"), "decision": v["decision"],
            "reasons": _json(v["reasons"]) or [], "note": v["note"] or "",
        })
    return [{"record": record_dict(records[i["record_id"]]), "status": i["status"],
             "votes": sorted(by_record.get(i["record_id"], []), key=lambda v: v["username"])}
            for i in open_items if i["record_id"] in records]


def summary(stage, user, role):
    out = {"stage": stage, "progress": progress(stage, user)}
    manage = role in MANAGE_ROLES
    if manage or not stage["blind_active"]:
        counts = rows("SELECT actor_id, count(*) AS n FROM current_vote WHERE stage_instance_id = %s::uuid GROUP BY 1",
                      [stage["id"]])
        names = usernames({c["actor_id"] for c in counts})
        out["reviewers"] = sorted(({"username": names.get(str(c["actor_id"]), "?"), "n_votes": c["n"]}
                                   for c in counts), key=lambda r: r["username"])
    if stage["blind_active"]:
        return out
    by_status = rows("SELECT status, decision, count(*) AS n FROM final_decision "
                     "WHERE stage_instance_id = %s::uuid GROUP BY 1, 2", [stage["id"]])
    touched = sum(r["n"] for r in by_status)
    out["decisions"] = {
        "include": sum(r["n"] for r in by_status if r["decision"] == "include"),
        "exclude": sum(r["n"] for r in by_status if r["decision"] == "exclude"),
        "maybe": sum(r["n"] for r in by_status if r["decision"] == "maybe"),
        "disagreement": sum(r["n"] for r in by_status if r["status"] == "disagreement"),
        "needs_clarification": sum(r["n"] for r in by_status if r["status"] == "needs_clarification"),
        "pending": sum(r["n"] for r in by_status if r["status"] == "pending") + stage["n_records"] - touched,
    }
    pairs = rows("SELECT * FROM cohen_kappa_pairs(%s::uuid)", [stage["id"]])
    names = usernames({p["rater_a"] for p in pairs} | {p["rater_b"] for p in pairs})
    for p in pairs:
        p["rater_a"], p["rater_b"] = names.get(str(p["rater_a"]), "?"), names.get(str(p["rater_b"]), "?")
    out["agreement"] = {
        "percent": one("SELECT * FROM percent_agreement(%s::uuid)", [stage["id"]]),
        "krippendorff": one("SELECT * FROM krippendorff_alpha(%s::uuid)", [stage["id"]]),
        "cohen_pairs": pairs,
    }
    return out


def export_rows(stage):
    decisions = {r["record_id"]: r for r in rows(
        "SELECT record_id, status, decision, n_votes FROM final_decision WHERE stage_instance_id = %s::uuid",
        [stage["id"]])}
    assignments = StageAssignment.objects.filter(stage_instance_id=stage["id"]).select_related("record").order_by("position")
    for a in assignments:
        d = decisions.get(str(a.record_id), {})
        r = a.record
        yield [str(r.id), r.title, r.authors, r.year or "", r.journal, r.doi,
               d.get("status", "pending"), d.get("decision") or "", d.get("n_votes", 0)]
