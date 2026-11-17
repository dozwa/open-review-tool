# Betrieb

Der Stack besteht aus fünf Diensten: `db` (PostgreSQL 17), `migrate` (läuft einmal je Start), `web` (Django),
`worker` (Hintergrundaufgaben) und `proxy` (Caddy). Nur der Proxy hat offene Ports (80 und 443).

## Einrichten

```bash
cp .env.example .env                      # ORT_DOMAIN und ORT_VERSION
mkdir -p secrets && chmod 700 secrets
openssl rand -hex 24 > secrets/db_password
openssl rand -hex 48 > secrets/django_secret_key
chmod 644 secrets/*
docker compose build
docker compose up -d
docker compose exec web python manage.py createsuperuser
```

Die Secret-Dateien müssen für den unprivilegierten Benutzer im Container lesbar sein, deshalb `644`.
Der Ordner selbst bleibt mit `700` vor anderen Benutzern des Servers geschützt.

Das Datenbankpasswort wird nur beim ersten Start übernommen. Ein späteres Ändern der Datei ändert das
Passwort in der Datenbank nicht.

## Zertifikate

- **Öffentliche Domain:** `ORT_DOMAIN` auf die Domain setzen. Caddy holt und erneuert das Zertifikat selbst.
  Voraussetzung: Die Domain zeigt auf den Server, und die Ports 80 und 443 sind von außen erreichbar.
- **localhost oder interner Name:** Caddy stellt ein Zertifikat aus einer eigenen lokalen Stelle aus.
  Browser zeigen eine Warnung, bis sie dieser Stelle vertrauen.
- **Server nur im Hochschulnetz:** Ein Zertifikat der Einrichtung muss in der `Caddyfile` eingetragen werden
  (`tls /pfad/cert.pem /pfad/key.pem`). Dieser Weg ist noch nicht getestet.

## Konten und Rollen

Der Administrator legt Konten unter `https://<Domain>/admin/` an. In einem Review vergibt der Owner die Rollen:

| Rolle | Darf |
| --- | --- |
| Owner, Admin | Importieren, Kriterien veröffentlichen, Stufen öffnen, entblinden und schließen, Mitglieder verwalten, abstimmen |
| Reviewer | Abstimmen, nach dem Entblinden Konflikte sehen und Konsens festhalten |
| Viewer | Lesen |

Solange eine Stufe verblindet ist, gibt die Schnittstelle niemandem fremde Stimmen heraus, auch dem Owner nicht.

## Sichern und Wiederherstellen

```bash
# Sichern
docker compose exec -T db pg_dump -Fc -U ort ort > ort-$(date +%F).dump

# Wiederherstellung prüfen: in eine zweite Datenbank einspielen und die Hash-Ketten prüfen
docker compose exec -T db createdb -U ort ort_restore
docker compose exec -T db pg_restore -U ort -d ort_restore --no-owner < ort-2026-11-17.dump
docker compose exec -T -e DATABASE_NAME=ort_restore web python manage.py verify_chain
docker compose exec -T db dropdb -U ort ort_restore
```

`verify_chain` prüft für jedes Review, dass die Ereignisse lückenlos sind und jeder Hash zum Vorgänger passt.
Der Befehl endet mit einem Fehler, sobald eine Kette beschädigt ist. Diese Abfolge läuft bei jedem Push in der CI.

## Aktualisieren

```bash
docker compose exec -T db pg_dump -Fc -U ort ort > vor-update.dump
git pull
docker compose build
docker compose up -d        # migrate läuft vor web und worker
```

Ein Wechsel der PostgreSQL-Hauptversion geht über Sichern und Wiederherstellen, nicht über den Tausch des
Image-Tags. Ab Version 18 liegt das Datenverzeichnis im Image an anderer Stelle.

## Prüfen

```bash
docker compose ps                                           # alle Dienste laufen, web ist healthy
docker compose logs --tail 100 web worker
pip install requests
python smoke_test.py https://localhost admin <Passwort>     # legt ein Test-Review und zwei Testkonten an
```

`smoke_test.py` schreibt Testdaten in die Installation. Es ist für eine frische oder eine Testinstallation gedacht.
