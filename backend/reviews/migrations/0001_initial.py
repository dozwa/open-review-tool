import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):
    initial = True

    dependencies = [
        ("eventcore", "0001_initial"),
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
    ]

    operations = [
        migrations.CreateModel(
            name="Actor",
            fields=[
                ("id", models.BigAutoField(auto_created=True, primary_key=True, serialize=False, verbose_name="ID")),
                ("actor_id", models.UUIDField(default=uuid.uuid4, editable=False, unique=True)),
                ("user", models.OneToOneField(on_delete=django.db.models.deletion.CASCADE, related_name="actor", to=settings.AUTH_USER_MODEL)),
            ],
        ),
        migrations.CreateModel(
            name="ImportBatch",
            fields=[
                ("id", models.UUIDField(default=uuid.uuid4, editable=False, primary_key=True, serialize=False)),
                ("filename", models.CharField(max_length=255)),
                ("fmt", models.CharField(max_length=16)),
                ("source", models.CharField(blank=True, max_length=255)),
                ("content", models.TextField()),
                ("sha256", models.CharField(max_length=64)),
                ("status", models.CharField(default="pending", max_length=16)),
                ("n_records", models.IntegerField(default=0)),
                ("n_duplicates", models.IntegerField(default=0)),
                ("error", models.TextField(blank=True)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("created_by", models.ForeignKey(on_delete=django.db.models.deletion.PROTECT, related_name="+", to=settings.AUTH_USER_MODEL)),
                ("review", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="imports", to="eventcore.review")),
            ],
        ),
        migrations.CreateModel(
            name="Membership",
            fields=[
                ("id", models.BigAutoField(auto_created=True, primary_key=True, serialize=False, verbose_name="ID")),
                ("role", models.CharField(max_length=16)),
                ("review", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="memberships", to="eventcore.review")),
                ("user", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="memberships", to=settings.AUTH_USER_MODEL)),
            ],
            options={
                "constraints": [models.UniqueConstraint(fields=("review", "user"), name="uniq_membership")],
            },
        ),
        migrations.CreateModel(
            name="Record",
            fields=[
                ("id", models.UUIDField(default=uuid.uuid4, editable=False, primary_key=True, serialize=False)),
                ("title", models.TextField(blank=True)),
                ("abstract", models.TextField(blank=True)),
                ("authors", models.TextField(blank=True)),
                ("year", models.IntegerField(blank=True, null=True)),
                ("journal", models.TextField(blank=True)),
                ("doi", models.CharField(blank=True, max_length=255)),
                ("pmid", models.CharField(blank=True, max_length=32)),
                ("url", models.TextField(blank=True)),
                ("keywords", models.TextField(blank=True)),
                ("raw", models.JSONField(default=dict)),
                ("dedup_key", models.CharField(max_length=300)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("batch", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="records", to="reviews.importbatch")),
                ("duplicate_of", models.ForeignKey(blank=True, null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="duplicates", to="reviews.record")),
                ("review", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="records", to="eventcore.review")),
            ],
            options={
                "indexes": [models.Index(fields=["review", "dedup_key"], name="record_review_dedup_idx")],
            },
        ),
        migrations.CreateModel(
            name="StageAssignment",
            fields=[
                ("id", models.BigAutoField(auto_created=True, primary_key=True, serialize=False, verbose_name="ID")),
                ("stage_instance_id", models.UUIDField(db_index=True)),
                ("position", models.IntegerField()),
                ("record", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="assignments", to="reviews.record")),
                ("review", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="+", to="eventcore.review")),
            ],
            options={
                "constraints": [models.UniqueConstraint(fields=("stage_instance_id", "record"), name="uniq_stage_record")],
            },
        ),
    ]
