"""Prüft die Oberfläche im Browser gegen den laufenden Stack und legt Bildschirmfotos ab.

Aufruf: python ui_check.py https://localhost admin <Passwort> <Ausgabeordner>
Voraussetzung: smoke_test.py lief vorher (es gibt ein Review "Smoke-Test ...").
"""
import pathlib
import re
import sys

from playwright.sync_api import expect, sync_playwright

BASE, USER, PASSWORD, OUT = sys.argv[1].rstrip("/"), sys.argv[2], sys.argv[3], pathlib.Path(sys.argv[4])
OUT.mkdir(parents=True, exist_ok=True)

with sync_playwright() as p:
    browser = p.chromium.launch()
    context = browser.new_context(ignore_https_errors=True, viewport={"width": 390, "height": 844})
    page = context.new_page()
    errors = []
    page.on("pageerror", lambda e: errors.append(str(e)))

    page.goto(BASE + "/")
    page.get_by_label("Benutzername").fill(USER)
    page.get_by_label("Passwort").fill(PASSWORD)
    page.screenshot(path=OUT / "1-anmeldung.png")
    page.get_by_role("button", name="Anmelden").click()

    expect(page.get_by_role("heading", name="Reviews")).to_be_visible()
    page.get_by_role("link", name=re.compile("Smoke-Test")).first.click()
    expect(page.get_by_role("heading", name="Stufen")).to_be_visible()
    page.screenshot(path=OUT / "2-review.png", full_page=True)

    # Neue Stufe über das Formular öffnen
    page.get_by_label("Name").fill("UI-Stufe")
    page.get_by_role("button", name="Stufe öffnen").click()
    expect(page.get_by_text("Stufe geöffnet.")).to_be_visible()
    page.get_by_role("link", name="Screenen").click()

    # Screening: Tastatur und Schaltfläche
    expect(page.get_by_text("0 von 4 bewertet")).to_be_visible()
    expect(page.locator("mark.inc").first).to_be_visible()  # Keyword-Hervorhebung
    page.screenshot(path=OUT / "3-screening.png", full_page=True)
    page.keyboard.press("i")
    expect(page.get_by_text("1 von 4 bewertet")).to_be_visible()
    page.get_by_role("button", name="Exclude (e)").click()
    expect(page.get_by_text("2 von 4 bewertet")).to_be_visible()

    # Auswertung: verblindet, dann entblinden
    page.get_by_role("link", name="Auswertung").click()
    expect(page.get_by_text("Die Stufe ist verblindet.")).to_be_visible()
    page.screenshot(path=OUT / "4-auswertung-verblindet.png", full_page=True)
    page.get_by_role("button", name="Entblinden").click()
    expect(page.get_by_role("heading", name="Entscheidungen")).to_be_visible()
    page.screenshot(path=OUT / "5-auswertung-offen.png", full_page=True)

    # Geschlossene Stufe aus dem Smoke-Test: Entscheidungen und Übereinstimmung
    page.get_by_role("link", name="← Review").click()
    page.get_by_role("listitem").filter(has_text="Titel/Abstract").get_by_role("link", name="Auswertung").click()
    expect(page.get_by_role("heading", name="Übereinstimmung")).to_be_visible()
    expect(page.get_by_text("75.0 %")).to_be_visible()
    page.screenshot(path=OUT / "6-auswertung-geschlossen.png", full_page=True)

    browser.close()
    if errors:
        raise SystemExit("JavaScript-Fehler im Browser:\n" + "\n".join(errors))
    print("OBERFLÄCHE GEPRÜFT")
