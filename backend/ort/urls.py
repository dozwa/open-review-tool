from django.conf import settings
from django.contrib import admin
from django.db import connection
from django.http import HttpResponse, JsonResponse
from django.urls import include, path, re_path
from django.views.decorators.csrf import ensure_csrf_cookie


def healthz(request):
    with connection.cursor() as cur:
        cur.execute("SELECT 1")
    return JsonResponse({"status": "ok"})


@ensure_csrf_cookie
def spa_index(request):
    """Liefert die gebaute Oberfläche für alle Pfade, die nicht API, Admin oder statisch sind."""
    index = settings.FRONTEND_DIST / "index.html"
    if not index.exists():
        return HttpResponse("Open Review Tool: Oberfläche ist nicht gebaut.", content_type="text/plain")
    return HttpResponse(index.read_text(encoding="utf-8"))


urlpatterns = [
    path("healthz", healthz),
    path("admin/", admin.site.urls),
    path("api/", include("reviews.urls")),
    re_path(r"^(?!api/|admin/|static/).*$", spa_index),
]
