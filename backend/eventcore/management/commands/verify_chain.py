from django.core.management.base import BaseCommand, CommandError

from eventcore import store
from eventcore.models import Review


class Command(BaseCommand):
    help = "Prüft die Hash-Kette aller Reviews, z. B. nach dem Einspielen einer Sicherung."

    def handle(self, *args, **options):
        bad = 0
        for review in Review.objects.order_by("title"):
            result = store.verify_chain(review.review_id)
            status = "OK" if result["ok"] else f"FEHLER ab seq {result['first_bad_seq']}"
            self.stdout.write(f"{review.review_id}  {result['n_events']:>7} Ereignisse  {status}  {review.title}")
            bad += 0 if result["ok"] else 1
        if bad:
            raise CommandError(f"{bad} Review(s) mit beschädigter Hash-Kette.")
        self.stdout.write("Alle Hash-Ketten sind unversehrt.")
