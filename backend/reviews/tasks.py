"""Hintergrundaufgaben. Definiert über die Django-Schnittstelle, damit das Backend tauschbar bleibt."""
try:
    from django.tasks import task
except ImportError:  # Django < 6.0: Backport
    from django_tasks import task


@task
def import_batch(batch_id: str) -> None:
    from . import services

    services.run_import(batch_id)
