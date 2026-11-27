#!/usr/bin/env python3
"""Capture real UI actions against an isolated, locally seeded demo only.

DEMO_URL=http://127.0.0.1:15374 DEMO_PASSWORD=... \
DEMO_DATABASE_PATH=/path/to/isolated/feedback-inbox.db python docs/capture-workflow.py
Requires Playwright and Chrome. No form submission or notification endpoint is used.
The local database path is required to restore status, timestamps and audit notes exactly.
Never run this against shared/production data or while other users are writing.
"""
import asyncio
import os
import tempfile
import sqlite3
from pathlib import Path
from playwright.async_api import async_playwright, expect
from frontend_style_audit import assert_computed_styles

URL = os.environ.get("DEMO_URL", "http://127.0.0.1:15374").rstrip("/")
OUT = Path(__file__).resolve().parent / "images"
NOTE = "已核對升級回饋並完成分類；交由團隊留存，本則不需對外回覆。"

async def settle(page):
    await page.evaluate("document.fonts.ready")
    await page.evaluate("Promise.all([...document.images].map(i => i.decode().catch(() => {})))")
    await page.evaluate("new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))")
    await assert_computed_styles(page)
    await page.mouse.move(0, 0)
    assert not await page.evaluate("document.documentElement.scrollWidth > innerWidth"), page.url

async def main():
    database = Path(os.environ["DEMO_DATABASE_PATH"]).resolve(strict=True)
    assert URL.startswith(("http://127.0.0.1:", "http://localhost:")), "Local isolated demo required"
    db = sqlite3.connect(database)
    db.row_factory = sqlite3.Row
    async with async_playwright() as p:
        options = {"channel": os.environ.get("PLAYWRIGHT_CHANNEL", "chrome")}
        if os.environ.get("PLAYWRIGHT_CHROMIUM_EXECUTABLE"):
            options = {"executable_path": os.environ["PLAYWRIGHT_CHROMIUM_EXECUTABLE"]}
        browser = await p.chromium.launch(**options)
        context = await browser.new_context(viewport={"width": 1440, "height": 1050}, color_scheme="light", timezone_id="Asia/Taipei")
        page = await context.new_page()
        errors = []
        page.on("pageerror", lambda e: errors.append(str(e)))
        async def get(path):
            response = await context.request.get(URL + path)
            assert response.ok, (path, response.status)
            return await response.json()
        await page.goto(URL, wait_until="networkidle")
        await page.get_by_label("帳號", exact=True).fill(os.environ.get("DEMO_USERNAME", "demo"))
        await page.get_by_label("密碼", exact=True).fill(os.environ["DEMO_PASSWORD"])
        await page.get_by_role("button", name="登入", exact=True).click()
        await expect(page.get_by_role("button", name="登出", exact=True)).to_be_visible()
        # Record only the authenticated workflow, never the login form.
        auth = await context.storage_state()
        await context.close()
        video_dir = tempfile.TemporaryDirectory(prefix="themed-motion-")
        context = await browser.new_context(
            storage_state=auth, viewport={"width": 1440, "height": 1050},
            color_scheme="light", timezone_id="Asia/Taipei",
            record_video_dir=video_dir.name, record_video_size={"width": 1440, "height": 1050},
        )
        page = await context.new_page()
        page.on("pageerror", lambda e: errors.append(str(e)))
        await page.goto(URL, wait_until="networkidle")
        stats = await get("/api/stats?tz_offset=480")
        assert stats["open_age"]["overdue"] > 0, "Seed the built-in demo first"
        envelope = page.get_by_role("link", name=f"已滿 7 天：{stats['open_age']['overdue']} 封，查看最舊來信")
        await envelope.focus()
        await page.keyboard.press("Enter")
        await expect(page.get_by_label("收到多久")).to_have_value("overdue")
        await expect(page.get_by_label("排序")).to_have_value("oldest")
        items = await get("/api/feedback?status=open&age=overdue&sort=oldest")
        assert items["total"] == stats["open_age"]["overdue"]
        dates = [item["created_at"] for item in items["items"]]
        assert dates == sorted(dates)
        item = items["items"][0]
        fid = item["id"]
        original = await get(f"/api/feedback/{fid}")
        row = dict(db.execute("SELECT * FROM feedback WHERE id=?", (fid,)).fetchone())
        assert row["created_at"] == original["created_at"] and row["status"] == original["status"]
        notes = [dict(n) for n in db.execute("SELECT * FROM notes WHERE feedback_id=? ORDER BY id", (fid,))]
        note_ids = {n["id"] for n in notes}
        await settle(page)
        row_link = page.locator(f'a[href="/inbox/{fid}"]').first
        await row_link.focus()
        checkbox = page.locator(f'.mail-row:has(a[href="/inbox/{fid}"]) input[type="checkbox"]')
        await checkbox.check()
        await page.wait_for_timeout(360)
        await checkbox.uncheck()
        await page.wait_for_timeout(360)
        await row_link.focus()
        await page.screenshot(path=str(OUT / "workflow-01.png"), animations="disabled")
        await page.locator(f'a[href="/inbox/{fid}"]').first.click()
        await expect(page.locator(".message-paper")).to_contain_text(original["message"])
        await expect(page).to_have_url(URL + f"/inbox/{fid}")
        await settle(page)
        await page.screenshot(path=str(OUT / "workflow-02.png"), animations="disabled")
        try:
            await page.get_by_label("狀態", exact=True).select_option("done")
            await expect(page.get_by_label("狀態", exact=True)).to_have_value("done")
            await expect(page.locator(".letter-state")).to_have_text("目前狀態 · 已完成")
            await page.wait_for_timeout(360)
            await page.get_by_label("新增備註", exact=True).fill(NOTE)
            await page.get_by_role("button", name="新增備註", exact=True).click()
            await expect(page.get_by_text(NOTE, exact=True)).to_be_visible()
            updated = await get(f"/api/feedback/{fid}")
            assert updated["id"] == fid and updated["message"] == original["message"]
            assert updated["status"] == "done"
            assert any(n["kind"] == "status" and n["body"] == "done" for n in updated["notes"] if n["id"] not in note_ids)
            assert sum(n["body"] == NOTE for n in updated["notes"]) == 1
            assert updated["deliveries"] == original["deliveries"], "No external messages"
            await settle(page)
            await page.screenshot(path=str(OUT / "workflow-03.png"), animations="disabled")
            # Finish the desktop demonstration before mobile viewport checks.
            desktop_video = page.video
            location = page.url
            auth = await context.storage_state()
            await context.close()
            await desktop_video.save_as(str(OUT / "feature-motion.webm"))
            context = await browser.new_context(storage_state=auth, viewport={"width": 1440, "height": 1050}, timezone_id="Asia/Taipei")
            page = await context.new_page()
            page.on("pageerror", lambda e: errors.append(str(e)))
            await page.goto(location, wait_until="networkidle")
            await page.emulate_media(reduced_motion="reduce")
            assert await page.locator(".message-paper").evaluate("e => getComputedStyle(e).animationName") == "none"
            assert await page.locator(".letter-state").evaluate("e => getComputedStyle(e).animationName") == "none"
            await page.set_viewport_size({"width": 390, "height": 844})
            await settle(page)
            await expect(page.get_by_text(NOTE, exact=True)).to_be_visible()
            await page.goto(URL + "/", wait_until="networkidle")
            await settle(page)
            await page.get_by_role("link", name="已滿 7 天", exact=False).click()
            await settle(page)
        finally:
            # Status audit entries cannot be deleted through the API. Restore only this
            # local demo record and the notes created by this run, even after failure.
            with db:
                current = list(db.execute("SELECT id FROM notes WHERE feedback_id=?", (fid,)))
                for note in current:
                    if note["id"] not in note_ids:
                        db.execute("DELETE FROM notes WHERE id=?", (note["id"],))
                db.execute("UPDATE feedback SET status=?, updated_at=? WHERE id=?", (row["status"], row["updated_at"], fid))
            assert dict(db.execute("SELECT * FROM feedback WHERE id=?", (fid,)).fetchone()) == row
            assert [dict(n) for n in db.execute("SELECT * FROM notes WHERE feedback_id=? ORDER BY id", (fid,))] == notes
            assert await get(f"/api/feedback/{fid}") == original
        assert not errors, errors
        db.close()
        await context.close()
        video_dir.cleanup()
        await browser.close()
        print(f"PASS: oldest-envelope drilldown → message #{fid} → persisted done + internal note; demo restored; desktop/mobile no overflow or page errors.")

asyncio.run(main())
