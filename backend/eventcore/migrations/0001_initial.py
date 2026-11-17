"""Legt den SQL-Kern an: Event-Log, Projektionen, Härtung. Die SQL-Dateien sind versioniert und
werden nach ihrer Auslieferung nicht mehr geändert; Änderungen kommen als neue Datei und Migration."""
import uuid
from pathlib import Path

import django.db.models.deletion
from django.db import migrations, models

SQL_DIR = Path(__file__).resolve().parent.parent / "sql"


def sql(name):
    return (SQL_DIR / name).read_text(encoding="utf-8")


REVERSE = """
DROP TABLE IF EXISTS snapshot, event, review CASCADE;
DROP FUNCTION IF EXISTS append_event(uuid, bigint, text, uuid, uuid, jsonb);
DROP FUNCTION IF EXISTS forbid_change();
DROP FUNCTION IF EXISTS stage_votes(uuid, text);
DROP FUNCTION IF EXISTS percent_agreement(uuid, text);
DROP FUNCTION IF EXISTS fleiss_kappa(uuid, text, int);
DROP FUNCTION IF EXISTS krippendorff_alpha(uuid, text);
DROP FUNCTION IF EXISTS cohen_kappa_pairs(uuid, text);
DROP FUNCTION IF EXISTS vote_matrix(uuid, text);
DROP FUNCTION IF EXISTS verify_chain(uuid);
"""


class Migration(migrations.Migration):
    initial = True
    dependencies = []

    operations = [
        migrations.RunSQL(sql("0001_schema.sql"), reverse_sql=REVERSE),
        migrations.RunSQL(sql("0002_projections.sql"), reverse_sql=migrations.RunSQL.noop),
        migrations.RunSQL(sql("0003_hardening.sql"), reverse_sql=migrations.RunSQL.noop),
        migrations.CreateModel(
            name="Review",
            fields=[
                ("review_id", models.UUIDField(default=uuid.uuid4, primary_key=True, serialize=False)),
                ("title", models.TextField()),
                ("last_seq", models.BigIntegerField(default=0)),
            ],
            options={"db_table": "review", "managed": False},
        ),
        migrations.CreateModel(
            name="Event",
            fields=[
                ("event_id", models.UUIDField(primary_key=True, serialize=False)),
                ("seq", models.BigIntegerField()),
                ("type", models.TextField()),
                ("schema_version", models.SmallIntegerField()),
                ("actor_id", models.UUIDField()),
                ("stage_instance_id", models.UUIDField(null=True)),
                ("occurred_at", models.DateTimeField()),
                ("payload", models.JSONField()),
                (
                    "review",
                    models.ForeignKey(
                        db_column="review_id",
                        on_delete=django.db.models.deletion.DO_NOTHING,
                        related_name="events",
                        to="eventcore.review",
                    ),
                ),
            ],
            options={"db_table": "event", "managed": False},
        ),
    ]
