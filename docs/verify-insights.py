#!/usr/bin/env python3
"""Exercise real insight drilldowns against an isolated seeded demo.

DEMO_URL=http://127.0.0.1:5173 DEMO_PASSWORD=... python docs/verify-insights.py
Requires Playwright and Chrome. No application data is changed.
"""
import asyncio, csv, io, os
from playwright.async_api import async_playwright, expect

URL=os.environ.get("DEMO_URL","http://127.0.0.1:5173").rstrip("/")
async def main():
    async with async_playwright() as p:
        browser=await p.chromium.launch(channel=os.environ.get("PLAYWRIGHT_CHANNEL","chrome"))
        context=await browser.new_context(viewport={"width":1440,"height":1050},timezone_id="Asia/Taipei",color_scheme="light")
        page=await context.new_page()
        errors=[]
        page.on("pageerror",lambda error:errors.append(str(error)))
        await page.goto(URL,wait_until="networkidle")
        await page.get_by_label("帳號",exact=True).fill(os.environ.get("DEMO_USERNAME","demo"))
        await page.get_by_label("密碼",exact=True).fill(os.environ["DEMO_PASSWORD"])
        await page.get_by_role("button",name="登入",exact=True).click()
        await expect(page.get_by_role("button",name="登出",exact=True)).to_be_visible()
        async def json_get(path):
            response=await context.request.get(URL+path)
            assert response.ok, (path,response.status)
            return await response.json()
        async def csv_count():
            href=await page.get_by_role("link",name="匯出 CSV",exact=True).get_attribute("href")
            response=await context.request.get(URL+href)
            assert response.ok
            return len(list(csv.reader(io.StringIO((await response.body()).decode("utf-8-sig")))))-1
        async def go(path):
            await page.goto(URL+path,wait_until="networkidle")
            assert not await page.evaluate("document.documentElement.scrollWidth > innerWidth"),path
        stats=await json_get("/api/stats?tz_offset=480")
        for age,label in [("overdue","已滿 7 天"),("waiting","1–7 天內"),("fresh","未滿 24 小時")]:
            await go("/")
            await page.get_by_role("link",name=f"{label}：{stats['open_age'][age]} 封，查看最舊來信").focus()
            await page.keyboard.press("Enter")
            await expect(page.get_by_label("收到多久")).to_have_value(age)
            await expect(page.get_by_label("排序")).to_have_value("oldest")
            result=await json_get(f"/api/feedback?status=open&age={age}&sort=oldest")
            assert result["total"]==stats["open_age"][age]
            assert await csv_count()==result["total"]
            dates=[r["created_at"] for r in result["items"]]
            assert dates==sorted(dates)
        day=stats["daily"][-1]
        await go("/")
        await page.get_by_role("link",name=f"{day['date']}，{day['count']} 封來信").click()
        await expect(page.get_by_label("從哪一天")).to_have_value(day["date"])
        assert await csv_count()==day["count"]
        for width,theme in [(390,"light"),(390,"dark"),(1440,"dark")]:
            await page.set_viewport_size({"width":width,"height":900})
            await page.evaluate("theme=>localStorage.setItem('fi.theme',theme)",theme)
            await go("/")
            assert await page.locator("html").evaluate("el=>el.classList.contains('dark')")==(theme=="dark")
            await page.get_by_role("link",name=f"已滿 7 天：{stats['open_age']['overdue']} 封，查看最舊來信").click()
            await expect(page.get_by_label("收到多久")).to_have_value("overdue")
            assert not await page.evaluate("document.documentElement.scrollWidth > innerWidth")
            assert await csv_count()==stats["open_age"]["overdue"]
        assert not errors, errors
        await browser.close()
        print("Real Chrome desktop/mobile/light/dark insight drilldowns, CSV counts and keyboard controls passed.")

asyncio.run(main())
