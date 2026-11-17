"""HTTP-Schnittstelle. Rechte und Verblindung werden hier erzwungen, nicht in der Oberfläche."""
import csv
import io
import json

from django.contrib.auth import authenticate, get_user_model, login, logout
from django.http import HttpResponse, JsonResponse
from django.middleware.csrf import get_token
from django.views.decorators.http import require_GET, require_POST
from rest_framework.decorators import api_view
from rest_framework.exceptions import NotFound, PermissionDenied, ValidationError
from rest_framework.response import Response
from rest_framework.views import exception_handler as drf_exception_handler

from eventcore import store
from eventcore.models import Review

from . import services, tasks
from .models import ImportBatch, Membership, Record
from .services import MANAGE_ROLES, WRITE_ROLES


def exception_handler(exc, context):
    if isinstance(exc, services.DomainError):
        return Response({"detail": exc.message}, status=exc.status)
    return drf_exception_handler(exc, context)


# ---------------------------------------------------------------- Anmeldung (ohne DRF, mit CSRF-Schutz)

def _me(user):
    return {"authenticated": True, "username": user.username, "is_staff": user.is_staff}


@require_GET
def me(request):
    get_token(request)  # stellt sicher, dass das CSRF-Cookie gesetzt wird
    if not request.user.is_authenticated:
        return JsonResponse({"authenticated": False})
    return JsonResponse(_me(request.user))


@require_POST
def login_view(request):
    try:
        data = json.loads(request.body or b"{}")
    except ValueError:
        data = {}
    user = authenticate(request, username=str(data.get("username", "")), password=str(data.get("password", "")))
    if user is None:
        return JsonResponse({"detail": "Benutzername oder Passwort ist falsch."}, status=400)
    login(request, user)
    return JsonResponse(_me(user))


@require_POST
def logout_view(request):
    logout(request)
    return JsonResponse({"authenticated": False})


@api_view(["POST"])
def users(request):
    """Legt ein Konto an. Nur für Administratoren der Installation."""
    if not request.user.is_staff:
        raise PermissionDenied("Nur Administratoren können Konten anlegen.")
    username = str(request.data.get("username", "")).strip()
    password = str(request.data.get("password", ""))
    if not username or len(password) < 8:
        raise ValidationError({"detail": "Benutzername fehlt oder Passwort ist kürzer als 8 Zeichen."})
    User = get_user_model()
    if User.objects.filter(username=username).exists():
        raise ValidationError({"detail": "Der Benutzername ist vergeben."})
    User.objects.create_user(username=username, password=password)
    return Response({"username": username}, status=201)


# ---------------------------------------------------------------- Hilfen

def member_role(request, review_id, allowed=None):
    membership = Membership.objects.filter(review_id=review_id, user=request.user).first()
    if membership is None:
        raise NotFound("Review nicht gefunden.")
    if allowed is not None and membership.role not in allowed:
        raise PermissionDenied("Dafür fehlt dir in diesem Review die Berechtigung.")
    return membership.role


def load_stage(request, stage_id, allowed=None):
    stage = services.get_stage(stage_id)
    return stage, member_role(request, stage["review_id"], allowed)


def require_unblinded(stage):
    if stage["blind_active"]:
        raise PermissionDenied("Die Stufe ist verblindet. Stimmen anderer sind erst nach dem Entblinden sichtbar.")


def import_dict(batch):
    return {"id": str(batch.id), "filename": batch.filename, "format": batch.fmt, "source": batch.source,
            "status": batch.status, "n_records": batch.n_records, "n_duplicates": batch.n_duplicates,
            "error": batch.error, "created_at": batch.created_at.isoformat()}


# ---------------------------------------------------------------- Reviews

@api_view(["GET", "POST"])
def reviews(request):
    if request.method == "POST":
        title = str(request.data.get("title", "")).strip()
        if not title:
            raise ValidationError({"detail": "Titel fehlt."})
        review = services.create_review(request.user, title)
        return Response({"id": str(review.review_id), "title": review.title, "role": "owner"}, status=201)
    memberships = Membership.objects.filter(user=request.user).select_related("review").order_by("review__title")
    return Response([{"id": str(m.review_id), "title": m.review.title, "role": m.role} for m in memberships])


@api_view(["GET"])
def review_detail(request, review_id):
    role = member_role(request, review_id)
    review = Review.objects.get(pk=review_id)
    members = Membership.objects.filter(review_id=review_id).select_related("user").order_by("user__username")
    records = Record.objects.filter(review_id=review_id)
    return Response({
        "id": str(review.review_id), "title": review.title, "role": role,
        "members": [{"username": m.user.username, "role": m.role} for m in members],
        "n_records": records.count(),
        "n_duplicates": records.filter(duplicate_of__isnull=False).count(),
        "criteria": services.current_criteria(review_id),
        "stages": services.list_stages(review_id),
        "imports": [import_dict(b) for b in ImportBatch.objects.filter(review_id=review_id).order_by("-created_at")],
    })


@api_view(["POST"])
def members(request, review_id):
    member_role(request, review_id, MANAGE_ROLES)
    services.set_member(review_id, request.user, str(request.data.get("username", "")).strip(),
                        str(request.data.get("role", "")))
    return Response({"ok": True})


@api_view(["PUT"])
def criteria(request, review_id):
    member_role(request, review_id, MANAGE_ROLES)
    return Response(services.publish_criteria(review_id, request.user, request.data))


@api_view(["GET", "POST"])
def imports(request, review_id):
    if request.method == "POST":
        member_role(request, review_id, MANAGE_ROLES)
        upload = request.FILES.get("file")
        if upload is None:
            raise ValidationError({"detail": "Datei fehlt."})
        text = upload.read().decode("utf-8-sig", errors="replace")
        try:
            batch = services.create_import(review_id, request.user, upload.name or "import", text,
                                           str(request.data.get("source", "")))
        except ValueError as exc:
            raise ValidationError({"detail": str(exc)})
        tasks.import_batch.enqueue(str(batch.id))
        batch.refresh_from_db()
        return Response(import_dict(batch), status=202)
    member_role(request, review_id)
    return Response([import_dict(b) for b in ImportBatch.objects.filter(review_id=review_id).order_by("-created_at")])


@api_view(["GET"])
def records(request, review_id):
    member_role(request, review_id)
    queryset = Record.objects.filter(review_id=review_id).order_by("title", "id")
    query = request.query_params.get("q", "").strip()
    if query:
        queryset = queryset.filter(title__icontains=query) | queryset.filter(abstract__icontains=query)
    if request.query_params.get("duplicates") == "0":
        queryset = queryset.filter(duplicate_of__isnull=True)
    try:
        limit = min(int(request.query_params.get("limit", 50)), 500)
        offset = max(int(request.query_params.get("offset", 0)), 0)
    except ValueError:
        raise ValidationError({"detail": "limit und offset müssen Zahlen sein."})
    return Response({"count": queryset.count(),
                     "results": [services.record_dict(r) for r in queryset[offset:offset + limit]]})


@api_view(["GET"])
def chain(request, review_id):
    member_role(request, review_id)
    return Response(store.verify_chain(review_id))


# ---------------------------------------------------------------- Stufen

@api_view(["POST"])
def stages(request, review_id):
    member_role(request, review_id, MANAGE_ROLES)
    d = request.data
    stage = services.open_stage(
        review_id, request.user, name=str(d.get("name", "")).strip(), kind=str(d.get("kind", "title_abstract")),
        blind=bool(d.get("blind", True)), min_reviewers=int(d.get("min_reviewers", 2)),
        maybe_as=d.get("maybe_as") or None, scope=str(d.get("scope", "all")),
        sample_size=d.get("sample_size"), seed=d.get("seed"), source_stage=d.get("source_stage"))
    return Response(stage, status=201)


@api_view(["GET"])
def stage_detail(request, stage_id):
    stage, role = load_stage(request, stage_id)
    return Response({"stage": stage, "role": role, "progress": services.progress(stage, request.user),
                     "criteria": services.current_criteria(stage["review_id"])})


@api_view(["GET"])
def stage_next(request, stage_id):
    stage, _ = load_stage(request, stage_id, WRITE_ROLES)
    return Response({"record": services.next_record(stage, request.user),
                     "progress": services.progress(stage, request.user)})


@api_view(["GET", "POST"])
def votes(request, stage_id):
    stage, _ = load_stage(request, stage_id, WRITE_ROLES)
    if request.method == "POST":
        d = request.data
        services.cast_vote(stage, request.user, d.get("record_id"), str(d.get("decision", "")),
                           d.get("reasons"), d.get("note", ""))
        return Response({"ok": True, "progress": services.progress(stage, request.user)}, status=201)
    mine = services.my_votes(stage, request.user)  # immer nur die eigenen Stimmen
    titles = dict(Record.objects.filter(pk__in=list(mine)).values_list("id", "title"))
    return Response([{"record_id": rid, "title": titles.get(_uuid(rid), ""), "decision": decision}
                     for rid, decision in mine.items()])


def _uuid(value):
    import uuid

    return uuid.UUID(str(value))


@api_view(["POST"])
def flags(request, stage_id):
    stage, _ = load_stage(request, stage_id, WRITE_ROLES)
    services.flag_undecidable(stage, request.user, request.data.get("record_id"), request.data.get("note", ""))
    return Response({"ok": True}, status=201)


@api_view(["POST"])
def unblind(request, stage_id):
    stage, _ = load_stage(request, stage_id, MANAGE_ROLES)
    services.unblind_stage(stage, request.user)
    return Response(services.get_stage(stage_id))


@api_view(["POST"])
def close(request, stage_id):
    stage, _ = load_stage(request, stage_id, MANAGE_ROLES)
    snapshot = services.close_stage(stage, request.user)
    return Response({"stage": services.get_stage(stage_id), "snapshot": snapshot})


@api_view(["GET"])
def conflicts(request, stage_id):
    stage, _ = load_stage(request, stage_id)
    require_unblinded(stage)
    return Response(services.conflicts(stage))


@api_view(["POST"])
def consensus(request, stage_id):
    stage, _ = load_stage(request, stage_id, WRITE_ROLES)
    d = request.data
    services.record_consensus(stage, request.user, d.get("record_id"), str(d.get("decision", "")),
                              str(d.get("method", "discussion")), d.get("reasons"), d.get("note", ""))
    return Response({"ok": True}, status=201)


@api_view(["GET"])
def stage_summary(request, stage_id):
    stage, role = load_stage(request, stage_id)
    return Response(services.summary(stage, request.user, role))


@api_view(["GET"])
def export_csv(request, stage_id):
    stage, _ = load_stage(request, stage_id)
    require_unblinded(stage)
    buffer = io.StringIO()
    writer = csv.writer(buffer)
    writer.writerow(["record_id", "title", "authors", "year", "journal", "doi", "status", "decision", "n_votes"])
    writer.writerows(services.export_rows(stage))
    response = HttpResponse(buffer.getvalue(), content_type="text/csv; charset=utf-8")
    response["Content-Disposition"] = f'attachment; filename="stage-{stage["id"]}.csv"'
    return response
