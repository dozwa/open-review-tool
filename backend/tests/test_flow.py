"""Durchgehender Ablauf über die API: Import, verblindete Abstimmung, Konflikt, Konsens, Abschluss."""
import io

import pytest
from django.contrib.auth import get_user_model
from rest_framework.test import APIClient

from eventcore.models import Event

RIS = """TY  - JOUR
AU  - Miller, Anna
AU  - Chen, Li
TI  - Deep learning for headache classification
AB  - We evaluate neural networks on clinical vignettes.
PY  - 2021
JO  - Journal of Tests
DO  - 10.1000/abc123
ER  -

TY  - JOUR
AU  - Novak, Petra
TI  - Rule-based triage of migraine patients in primary care
AB  - A prospective cohort study.
PY  - 2019
JO  - Primary Care Research
ER  -

TY  - JOUR
AU  - Okafor, Chidi
TI  - Knowledge graphs in neurology: a scoping review
PY  - 2022
JO  - Neuro Informatics
DO  - 10.1000/kg2022
ER  -

TY  - JOUR
AU  - Miller, A.
TI  - Deep Learning for Headache Classification.
PY  - 2021
DO  - https://doi.org/10.1000/ABC123
ER  -
"""

BIBTEX = """@article{novak2019,
  title = {Rule-based triage of {Migraine} patients in primary care},
  author = {Novak, Petra and Svoboda, Jan},
  year = {2019},
  journal = {Primary Care Research}
}
@article{lee2020,
  title = {Large language models for symptom extraction},
  author = {Lee, Min},
  year = {2020},
  doi = {10.1000/llm2020}
}
"""


def make_client(username, staff=False):
    user = get_user_model().objects.create_user(username=username, password="geheim-geheim", is_staff=staff)
    client = APIClient()
    assert client.login(username=username, password="geheim-geheim")
    client.user = user
    return client


def upload(client, review_id, name, text):
    return client.post(f"/api/reviews/{review_id}/imports",
                       {"file": io.BytesIO(text.encode()), "source": "Test"}, format="multipart")


@pytest.fixture
def team(db):
    return {name: make_client(name, staff=(name == "owner"))
            for name in ("owner", "r1", "r2", "viewer", "outsider")}


@pytest.fixture
def review(team):
    owner = team["owner"]
    response = owner.post("/api/reviews", {"title": "Testreview"}, format="json")
    assert response.status_code == 201, response.content
    review_id = response.json()["id"]
    for name, role in (("r1", "reviewer"), ("r2", "reviewer"), ("viewer", "viewer")):
        r = owner.post(f"/api/reviews/{review_id}/members", {"username": name, "role": role}, format="json")
        assert r.status_code == 200, r.content
    # RIS: 4 Einträge, der vierte ist eine DOI-Dublette des ersten
    r = upload(owner, review_id, "suche.ris", RIS)
    assert r.status_code == 202, r.content
    assert r.json()["status"] == "done", r.json()
    assert (r.json()["n_records"], r.json()["n_duplicates"]) == (4, 1)
    # BibTeX: 2 Einträge, der erste ist eine Titel-Dublette (ohne DOI)
    r = upload(owner, review_id, "suche.bib", BIBTEX)
    assert r.json()["status"] == "done", r.json()
    assert (r.json()["n_records"], r.json()["n_duplicates"]) == (2, 1)
    return review_id


def open_stage(client, review_id, **extra):
    body = {"name": "Titel/Abstract", "blind": True, "min_reviewers": 2, "scope": "all", **extra}
    r = client.post(f"/api/reviews/{review_id}/stages", body, format="json")
    assert r.status_code == 201, r.content
    return r.json()


def test_login_and_me(db):
    get_user_model().objects.create_user(username="anna", password="geheim-geheim")
    client = APIClient()
    assert client.get("/api/auth/me").json() == {"authenticated": False}
    assert client.post("/api/auth/login", {"username": "anna", "password": "falsch"}, format="json").status_code == 400
    r = client.post("/api/auth/login", {"username": "anna", "password": "geheim-geheim"}, format="json")
    assert r.status_code == 200 and r.json()["username"] == "anna"
    assert client.get("/api/auth/me").json()["authenticated"] is True
    assert client.get("/api/reviews").json() == []


def test_unauthenticated_and_outsider(team, review):
    assert APIClient().get("/api/reviews").status_code == 403
    assert team["outsider"].get(f"/api/reviews/{review}").status_code == 404
    assert team["outsider"].get("/api/reviews").json() == []
    assert team["r1"].post("/api/users", {"username": "x", "password": "geheim-geheim"}, format="json").status_code == 403


def test_review_detail_and_records(team, review):
    detail = team["viewer"].get(f"/api/reviews/{review}").json()
    assert detail["role"] == "viewer"
    assert detail["n_records"] == 6 and detail["n_duplicates"] == 2
    assert {m["username"]: m["role"] for m in detail["members"]}["owner"] == "owner"
    unique = team["r1"].get(f"/api/reviews/{review}/records?duplicates=0").json()
    assert unique["count"] == 4
    hits = team["r1"].get(f"/api/reviews/{review}/records?q=migraine").json()
    assert hits["count"] == 2  # Original und Dublette
    # Rechte: Reviewer dürfen weder importieren noch Stufen öffnen
    assert upload(team["r1"], review, "x.ris", RIS).status_code == 403
    assert team["r1"].post(f"/api/reviews/{review}/stages", {"name": "x"}, format="json").status_code == 403


def test_criteria_versions(team, review):
    owner = team["owner"]
    r = owner.put(f"/api/reviews/{review}/criteria", {
        "text": "PCC ...", "include_keywords": "headache, migraine", "exclude_keywords": ["animal"],
        "exclusion_reasons": ["Falsche Population", "Kein Primärstudie"]}, format="json")
    assert r.status_code == 200 and r.json()["version"] == 1
    assert r.json()["include_keywords"] == ["headache", "migraine"]
    assert owner.put(f"/api/reviews/{review}/criteria", {"text": "v2"}, format="json").json()["version"] == 2
    assert team["r1"].put(f"/api/reviews/{review}/criteria", {"text": "x"}, format="json").status_code == 403
    assert team["r1"].get(f"/api/reviews/{review}").json()["criteria"]["version"] == 2


def test_blind_screening_conflict_consensus_close(team, review):
    owner, r1, r2, viewer = team["owner"], team["r1"], team["r2"], team["viewer"]
    stage = open_stage(owner, review)
    sid = stage["id"]
    assert stage["n_records"] == 4 and stage["blind_active"] is True

    # Beide sehen dieselbe Reihenfolge und stimmen alle vier Records ab.
    seen = []
    plan = {"r1": ["include", "exclude", "include", "maybe"], "r2": ["exclude", "exclude", "include", "include"]}
    for name, client in (("r1", r1), ("r2", r2)):
        order = []
        for decision in plan[name]:
            nxt = client.get(f"/api/stages/{sid}/next").json()
            assert nxt["record"] is not None
            order.append(nxt["record"]["id"])
            r = client.post(f"/api/stages/{sid}/votes", {
                "record_id": nxt["record"]["id"], "decision": decision,
                "reasons": ["Falsche Population"] if decision == "exclude" else [], "note": ""}, format="json")
            assert r.status_code == 201, r.content
        assert client.get(f"/api/stages/{sid}/next").json() == {"record": None, "progress": {"done": 4, "total": 4}}
        seen.append(order)
    assert seen[0] == seen[1]
    rec_a, rec_b, rec_c, rec_d = seen[0]

    # Viewer dürfen nicht abstimmen.
    assert viewer.post(f"/api/stages/{sid}/votes", {"record_id": rec_a, "decision": "include"},
                       format="json").status_code == 403
    # Verblindet: niemand bekommt fremde Stimmen, auch nicht der Owner.
    for client in (owner, r1, r2, viewer):
        assert client.get(f"/api/stages/{sid}/conflicts").status_code == 403
        assert client.get(f"/api/stages/{sid}/export.csv").status_code == 403
        assert "decisions" not in client.get(f"/api/stages/{sid}/summary").json()
    assert "reviewers" not in r1.get(f"/api/stages/{sid}/summary").json()
    assert [x["n_votes"] for x in owner.get(f"/api/stages/{sid}/summary").json()["reviewers"]] == [4, 4]
    # Eigene Stimmen bleiben sichtbar.
    assert len(r1.get(f"/api/stages/{sid}/votes").json()) == 4
    # Konsens ist verblindet nicht möglich.
    assert r1.post(f"/api/stages/{sid}/consensus", {"record_id": rec_a, "decision": "include"},
                   format="json").status_code == 403

    # Entblinden darf nur Owner/Admin.
    assert r1.post(f"/api/stages/{sid}/unblind").status_code == 403
    assert owner.post(f"/api/stages/{sid}/unblind").status_code == 200

    conflicts = r1.get(f"/api/stages/{sid}/conflicts").json()
    assert {c["record"]["id"] for c in conflicts} == {rec_a, rec_d}  # include/exclude und maybe/include
    conflict_a = next(c for c in conflicts if c["record"]["id"] == rec_a)
    assert [(v["username"], v["decision"]) for v in conflict_a["votes"]] == [("r1", "include"), ("r2", "exclude")]
    assert conflict_a["votes"][1]["reasons"] == ["Falsche Population"]

    summary = r1.get(f"/api/stages/{sid}/summary").json()
    assert summary["decisions"] == {"include": 1, "exclude": 1, "maybe": 0, "disagreement": 2,
                                    "needs_clarification": 0, "pending": 0}
    assert summary["agreement"]["percent"]["n_items"] == 4
    assert summary["agreement"]["percent"]["percent_agreement"] == pytest.approx(0.5)
    assert len(summary["agreement"]["cohen_pairs"]) == 1

    # Konsens für A, Revision der eigenen Stimme für D.
    assert r2.post(f"/api/stages/{sid}/consensus", {"record_id": rec_a, "decision": "include",
                                                    "method": "discussion"}, format="json").status_code == 201
    assert r1.post(f"/api/stages/{sid}/votes", {"record_id": rec_d, "decision": "include"},
                   format="json").status_code == 201
    assert Event.objects.filter(stage_instance_id=sid, type="VoteRevised").count() == 1
    assert r1.get(f"/api/stages/{sid}/conflicts").json() == []
    summary = owner.get(f"/api/stages/{sid}/summary").json()
    assert summary["decisions"]["include"] == 3 and summary["decisions"]["exclude"] == 1

    # CSV-Export
    csv_text = owner.get(f"/api/stages/{sid}/export.csv").content.decode()
    assert csv_text.splitlines()[0].startswith("record_id,title") and len(csv_text.splitlines()) == 5

    # Abschluss: Snapshot, danach keine Stimmen mehr.
    assert r1.post(f"/api/stages/{sid}/close").status_code == 403
    closed = owner.post(f"/api/stages/{sid}/close").json()
    assert closed["stage"]["closed"] is True and len(closed["snapshot"]["hash"]) == 64
    assert r1.post(f"/api/stages/{sid}/votes", {"record_id": rec_b, "decision": "include"},
                   format="json").status_code == 409

    # Folgestufe: nur die eingeschlossenen Records.
    fulltext = open_stage(owner, review, name="Volltext", kind="fulltext", scope="from_stage", source_stage=sid)
    assert fulltext["n_records"] == 3

    # Hash-Kette ist unversehrt.
    chain = viewer.get(f"/api/reviews/{review}/chain").json()
    assert chain["ok"] is True and chain["n_events"] > 15


def test_pilot_sample_is_reproducible(team, review):
    owner = team["owner"]
    pilot = open_stage(owner, review, name="Pilot", kind="pilot", scope="sample", sample_size=2, seed=7)
    again = open_stage(owner, review, name="Pilot 2", kind="pilot", scope="sample", sample_size=2, seed=7)
    assert pilot["n_records"] == 2 and pilot["seed"] == 7
    first = team["r1"].get(f"/api/stages/{pilot['id']}/next").json()["record"]["id"]
    second = team["r1"].get(f"/api/stages/{again['id']}/next").json()["record"]["id"]
    assert first == second


def test_undecidable_flag_goes_to_clarification(team, review):
    owner, r1 = team["owner"], team["r1"]
    stage = open_stage(owner, review, blind=False)
    record = r1.get(f"/api/stages/{stage['id']}/next").json()["record"]["id"]
    assert r1.post(f"/api/stages/{stage['id']}/flags", {"record_id": record, "note": "Abstract fehlt"},
                   format="json").status_code == 201
    conflicts = r1.get(f"/api/stages/{stage['id']}/conflicts").json()
    assert [(c["record"]["id"], c["status"]) for c in conflicts] == [(record, "needs_clarification")]


def test_failed_import_is_reported(team, review):
    r = upload(team["owner"], review, "leer.ris", "Diese Datei enthält keine Literaturangaben.\n")
    assert r.status_code == 202 and r.json()["status"] == "failed" and r.json()["error"]
    assert upload(team["owner"], review, "bild.png", "kein Literaturformat").status_code == 400
