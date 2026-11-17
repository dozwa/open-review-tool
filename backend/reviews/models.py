"""Stammdaten. Fachliche Entscheidungen stehen nicht hier, sondern im Event-Log."""
import uuid

from django.conf import settings
from django.db import models


class Actor(models.Model):
    """Pseudonyme ID einer Person. Im Event-Log steht nur die actor_id, nie der Name."""

    user = models.OneToOneField(settings.AUTH_USER_MODEL, on_delete=models.CASCADE, related_name="actor")
    actor_id = models.UUIDField(default=uuid.uuid4, unique=True, editable=False)


class Membership(models.Model):
    ROLES = ("owner", "admin", "reviewer", "viewer")

    review = models.ForeignKey("eventcore.Review", on_delete=models.CASCADE, related_name="memberships")
    user = models.ForeignKey(settings.AUTH_USER_MODEL, on_delete=models.CASCADE, related_name="memberships")
    role = models.CharField(max_length=16)

    class Meta:
        constraints = [models.UniqueConstraint(fields=["review", "user"], name="uniq_membership")]


class ImportBatch(models.Model):
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    review = models.ForeignKey("eventcore.Review", on_delete=models.CASCADE, related_name="imports")
    filename = models.CharField(max_length=255)
    fmt = models.CharField(max_length=16)
    source = models.CharField(max_length=255, blank=True)
    content = models.TextField()
    sha256 = models.CharField(max_length=64)
    status = models.CharField(max_length=16, default="pending")
    n_records = models.IntegerField(default=0)
    n_duplicates = models.IntegerField(default=0)
    error = models.TextField(blank=True)
    created_by = models.ForeignKey(settings.AUTH_USER_MODEL, on_delete=models.PROTECT, related_name="+")
    created_at = models.DateTimeField(auto_now_add=True)


class Record(models.Model):
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    review = models.ForeignKey("eventcore.Review", on_delete=models.CASCADE, related_name="records")
    batch = models.ForeignKey(ImportBatch, on_delete=models.CASCADE, related_name="records")
    title = models.TextField(blank=True)
    abstract = models.TextField(blank=True)
    authors = models.TextField(blank=True)
    year = models.IntegerField(null=True, blank=True)
    journal = models.TextField(blank=True)
    doi = models.CharField(max_length=255, blank=True)
    pmid = models.CharField(max_length=32, blank=True)
    url = models.TextField(blank=True)
    keywords = models.TextField(blank=True)
    raw = models.JSONField(default=dict)
    dedup_key = models.CharField(max_length=300)
    duplicate_of = models.ForeignKey("self", null=True, blank=True, on_delete=models.SET_NULL, related_name="duplicates")
    created_at = models.DateTimeField(auto_now_add=True)

    class Meta:
        indexes = [models.Index(fields=["review", "dedup_key"], name="record_review_dedup_idx")]


class StageAssignment(models.Model):
    """Welche Records zu einer Stufeninstanz gehören und in welcher Reihenfolge."""

    stage_instance_id = models.UUIDField(db_index=True)
    review = models.ForeignKey("eventcore.Review", on_delete=models.CASCADE, related_name="+")
    record = models.ForeignKey(Record, on_delete=models.CASCADE, related_name="assignments")
    position = models.IntegerField()

    class Meta:
        constraints = [models.UniqueConstraint(fields=["stage_instance_id", "record"], name="uniq_stage_record")]
