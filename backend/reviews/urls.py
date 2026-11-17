from django.urls import path

from . import api

urlpatterns = [
    path("auth/me", api.me),
    path("auth/login", api.login_view),
    path("auth/logout", api.logout_view),
    path("users", api.users),
    path("reviews", api.reviews),
    path("reviews/<uuid:review_id>", api.review_detail),
    path("reviews/<uuid:review_id>/members", api.members),
    path("reviews/<uuid:review_id>/criteria", api.criteria),
    path("reviews/<uuid:review_id>/imports", api.imports),
    path("reviews/<uuid:review_id>/records", api.records),
    path("reviews/<uuid:review_id>/chain", api.chain),
    path("reviews/<uuid:review_id>/stages", api.stages),
    path("stages/<uuid:stage_id>", api.stage_detail),
    path("stages/<uuid:stage_id>/next", api.stage_next),
    path("stages/<uuid:stage_id>/votes", api.votes),
    path("stages/<uuid:stage_id>/flags", api.flags),
    path("stages/<uuid:stage_id>/unblind", api.unblind),
    path("stages/<uuid:stage_id>/close", api.close),
    path("stages/<uuid:stage_id>/conflicts", api.conflicts),
    path("stages/<uuid:stage_id>/consensus", api.consensus),
    path("stages/<uuid:stage_id>/summary", api.stage_summary),
    path("stages/<uuid:stage_id>/export.csv", api.export_csv),
]
