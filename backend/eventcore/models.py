"""Lesemodelle auf dem SQL-Kern. Die Tabellen legt die SQL-Migration an (managed = False)."""
import uuid

from django.db import models


class Review(models.Model):
    review_id = models.UUIDField(primary_key=True, default=uuid.uuid4)
    title = models.TextField()
    last_seq = models.BigIntegerField(default=0)

    class Meta:
        managed = False
        db_table = "review"

    def __str__(self):
        return self.title


class Event(models.Model):
    """Nur zum Lesen. Geschrieben wird ausschließlich über eventcore.store.append()."""

    event_id = models.UUIDField(primary_key=True)
    review = models.ForeignKey(Review, on_delete=models.DO_NOTHING, db_column="review_id", related_name="events")
    seq = models.BigIntegerField()
    type = models.TextField()
    schema_version = models.SmallIntegerField()
    actor_id = models.UUIDField()
    stage_instance_id = models.UUIDField(null=True)
    occurred_at = models.DateTimeField()
    payload = models.JSONField()

    class Meta:
        managed = False
        db_table = "event"
