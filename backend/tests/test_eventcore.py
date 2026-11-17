"""Der SQL-Kern, erreicht über die Django-Migrationen."""
import uuid

import pytest
from django.db import DatabaseError, connection, transaction

from eventcore import store
from eventcore.models import Event, Review

ACTOR = uuid.uuid4()


@pytest.fixture
def review(db):
    return Review.objects.create(title="Kern")


def test_append_assigns_sequence_and_chain(review):
    assert store.append(review.review_id, "ReviewCreated", ACTOR, {"title": "Kern"}) == 1
    assert store.append(review.review_id, "MemberRoleSet", ACTOR, {"role": "owner"}) == 2
    events = list(Event.objects.filter(review_id=review.review_id).order_by("seq"))
    assert [e.type for e in events] == ["ReviewCreated", "MemberRoleSet"]
    assert events[0].payload == {"title": "Kern"}
    assert store.verify_chain(review.review_id) == {"n_events": 2, "n_bad": 0, "first_bad_seq": None, "ok": True}


def test_stale_sequence_is_rejected(review):
    store.append_expected(review.review_id, 0, "A", ACTOR, {})
    with pytest.raises(store.ConcurrencyError):
        store.append_expected(review.review_id, 0, "B", ACTOR, {})  # zweiter Schreiber mit altem Stand
    assert Event.objects.filter(review_id=review.review_id).count() == 1
    assert store.append(review.review_id, "B", ACTOR, {}) == 2


def test_events_are_immutable(review):
    store.append(review.review_id, "A", ACTOR, {})
    for statement in ("UPDATE event SET type = 'X'", "DELETE FROM event", "TRUNCATE event CASCADE"):
        with pytest.raises(DatabaseError):
            with transaction.atomic():
                with connection.cursor() as cur:
                    cur.execute(statement)
    assert Event.objects.filter(review_id=review.review_id).count() == 1


def test_tampering_is_detected(review):
    for i in range(3):
        store.append(review.review_id, "A", ACTOR, {"i": i})
    with connection.cursor() as cur:
        cur.execute("ALTER TABLE event DISABLE TRIGGER event_immutable")
        cur.execute("UPDATE event SET payload = '{\"i\": 99}' WHERE review_id = %s AND seq = 2", [review.review_id])
        cur.execute("ALTER TABLE event ENABLE TRIGGER event_immutable")
    result = store.verify_chain(review.review_id)
    assert result["ok"] is False and result["first_bad_seq"] == 2


def test_unknown_review(db):
    with pytest.raises(ValueError):
        store.append(uuid.uuid4(), "A", ACTOR, {})
