from django.contrib import admin

from .models import ImportBatch, Membership


@admin.register(Membership)
class MembershipAdmin(admin.ModelAdmin):
    list_display = ("review", "user", "role")
    list_filter = ("role",)


@admin.register(ImportBatch)
class ImportBatchAdmin(admin.ModelAdmin):
    list_display = ("filename", "review", "status", "n_records", "n_duplicates", "created_at")
    exclude = ("content",)
