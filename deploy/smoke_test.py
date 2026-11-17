"""Durchgehender Test gegen den laufenden Compose-Stack, durch den Proxy hindurch.

Aufruf: python smoke_test.py https://localhost admin <Passwort>
Prüft Anmeldung, Import über den Worker, verblindete Abstimmung, Konsens, Abschluss und Hash-Kette.
"""
import pathlib
import sys
import time

import requests
import urllib3

urllib3.disable_warnings()  # lokales Zertifikat des Proxys
BASE, ADMIN, ADMIN_PASSWORD = sys.argv[1].rstrip("/"), sys.argv[2], sys.argv[3]
PASSWORD = "smoke-test-passwort"
SAMPLE = pathlib.Path(__file__).parent / "sample" / "sample.ris"


class Client:
    def __init__(self, username, password):
        self.s = requests.Session()
        self.s.verify = False
        self.s.headers["Referer"] = BASE + "/"
        self.get("/api/auth/me")
        result = self.post("/api/auth/login", json={"username": username, "password": password})
        assert result["username"] == username

    def _check(self, r, expect):
        if r.status_code != expect:
            raise SystemExit(f"{r.request.method} {r.url}: {r.status_code} statt {expect}\n{r.text[:2000]}")
        return r.json() if "json" in r.headers.get("content-type", "") else r.text

    def get(self, path, expect=200):
        return self._check(self.s.get(BASE + path), expect)

    def post(self, path, expect=None, **kwargs):
        headers = {"X-CSRFToken": self.s.cookies.get("csrftoken", "")}
        r = self.s.post(BASE + path, headers=headers, **kwargs)
        if expect is None:
            expect = r.status_code if r.status_code in (200, 201, 202) else 200
        return self._check(r, expect)


def main():
    for _ in range(60):
        try:
            if requests.get(BASE + "/healthz", verify=False, timeout=5).status_code == 200:
                break
        except requests.RequestException:
            pass
        time.sleep(3)
    else:
        raise SystemExit("Der Stack wurde nicht gesund.")
    print("Stack ist erreichbar.")

    page = requests.get(BASE + "/", verify=False)
    assert page.status_code == 200 and "<div id=\"root\">" in page.text, "Oberfläche wird nicht ausgeliefert"
    print("Oberfläche wird ausgeliefert.")

    admin = Client(ADMIN, ADMIN_PASSWORD)
    suffix = str(int(time.time()))
    names = [f"r1-{suffix}", f"r2-{suffix}"]
    for name in names:
        admin.post("/api/users", expect=201, json={"username": name, "password": PASSWORD})
    review = admin.post("/api/reviews", expect=201, json={"title": f"Smoke-Test {suffix}"})["id"]
    for name in names:
        admin.post(f"/api/reviews/{review}/members", json={"username": name, "role": "reviewer"})
    admin.s.put(BASE + f"/api/reviews/{review}/criteria", headers={"X-CSRFToken": admin.s.cookies["csrftoken"]},
                json={"include_keywords": ["headache", "migraine"], "exclude_keywords": ["animal"],
                      "exclusion_reasons": ["Falsche Population"]}).raise_for_status()

    with SAMPLE.open("rb") as f:
        batch = admin.post(f"/api/reviews/{review}/imports", expect=202,
                           files={"file": ("sample.ris", f)}, data={"source": "Smoke"})
    for _ in range(40):  # der Worker importiert im Hintergrund
        batch = next(b for b in admin.get(f"/api/reviews/{review}/imports") if b["id"] == batch["id"])
        if batch["status"] in ("done", "failed"):
            break
        time.sleep(2)
    assert batch["status"] == "done", batch
    assert (batch["n_records"], batch["n_duplicates"]) == (5, 1), batch
    print("Import über den Worker: 5 Einträge, 1 Dublette.")

    stage = admin.post(f"/api/reviews/{review}/stages", expect=201,
                       json={"name": "Titel/Abstract", "blind": True, "min_reviewers": 2})["id"]
    r1, r2 = (Client(name, PASSWORD) for name in names)
    plan = {r1: ["include", "exclude", "include", "exclude"], r2: ["exclude", "exclude", "include", "exclude"]}
    first = None
    for client, decisions in plan.items():
        for decision in decisions:
            record = client.get(f"/api/stages/{stage}/next")["record"]
            first = first or record["id"]
            client.post(f"/api/stages/{stage}/votes", expect=201,
                        json={"record_id": record["id"], "decision": decision})
    r1.get(f"/api/stages/{stage}/conflicts", expect=403)
    admin.get(f"/api/stages/{stage}/conflicts", expect=403)
    print("Verblindet: fremde Stimmen sind gesperrt.")

    admin.post(f"/api/stages/{stage}/unblind", expect=200)
    conflicts = r1.get(f"/api/stages/{stage}/conflicts")
    assert [c["record"]["id"] for c in conflicts] == [first], conflicts
    r2.post(f"/api/stages/{stage}/consensus", expect=201, json={"record_id": first, "decision": "include"})
    summary = admin.get(f"/api/stages/{stage}/summary")
    assert summary["decisions"]["include"] == 2 and summary["decisions"]["exclude"] == 2, summary
    assert abs(summary["agreement"]["percent"]["percent_agreement"] - 0.75) < 1e-9, summary
    closed = admin.post(f"/api/stages/{stage}/close", expect=200)
    assert closed["stage"]["closed"] and len(closed["snapshot"]["hash"]) == 64
    chain = admin.get(f"/api/reviews/{review}/chain")
    assert chain["ok"], chain
    print(f"Konflikt gelöst, Stufe geschlossen, Hash-Kette über {chain['n_events']} Ereignisse unversehrt.")
    print("SMOKE-TEST BESTANDEN")


if __name__ == "__main__":
    main()
