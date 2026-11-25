import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, test } from "vitest";
import { fakeApi } from "../test/fakeApi";
import { admin, detail, item, member, meta, site } from "../test/fixtures";
import { renderApp } from "../test/render";
import type { FeedbackDetail } from "../types";

const signedIn = (user = admin) => ({
  "GET /meta": () => meta,
  "GET /auth/state": () => ({ setup_required: false, user }),
  "GET /sites": () => [site],
});

test("網址上的篩選條件會送給後端，匯出連結也帶著", async () => {
  const queries: string[] = [];
  fakeApi({
    ...signedIn(),
    "GET /feedback": (_body, url) => {
      queries.push(url.search);
      return { items: [item(1)], total: 1, page: 1, per_page: 25 };
    },
  });
  renderApp("/inbox?status=open&site_id=7&q=報價");

  expect(await screen.findByText("第 1 則訊息")).toBeInTheDocument();
  const sent = new URLSearchParams(queries[0]);
  expect(sent.get("status")).toBe("open");
  expect(sent.get("site_id")).toBe("7");
  expect(sent.get("q")).toBe("報價");
  expect(screen.getByRole("tab", { name: "待處理" })).toHaveAttribute("aria-selected", "true");

  const exportHref = screen.getByRole("link", { name: "匯出 CSV" }).getAttribute("href") ?? "";
  expect(exportHref).toMatch(/^\/api\/feedback\/export\?/);
  const exported = new URLSearchParams(exportHref.split("?")[1]);
  expect(exported.get("status")).toBe("open");
  expect(exported.has("tz_offset")).toBe(true);
  expect(exported.has("page")).toBe(false);
});

test("勾選幾則之後可以一次改狀態", async () => {
  const calls = fakeApi({
    ...signedIn(),
    "GET /feedback": () => ({ items: [item(1), item(2), item(3)], total: 3, page: 1, per_page: 25 }),
    "POST /feedback/bulk": () => ({ updated: 2 }),
  });
  renderApp("/inbox");

  await userEvent.click(await screen.findByLabelText("選取 訪客 1 的訊息"));
  await userEvent.click(screen.getByLabelText("選取 訪客 3 的訊息"));
  expect(screen.getByText("選了 2 則")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "已完成" }));

  expect(await screen.findByRole("status")).toHaveTextContent("2 則改成「已完成」");
  expect(calls.find((c) => c.path === "/feedback/bulk")?.body).toEqual({ ids: [1, 3], status: "done" });
  // 改完重新載入，勾選也清掉
  await waitFor(() => expect(screen.getByText("共 3 則")).toBeInTheDocument());
});

test("訊息頁可以改狀態、加備註，成員看不到刪除", async () => {
  let current: FeedbackDetail = detail;
  const calls = fakeApi({
    ...signedIn(member),
    "GET /feedback/5": () => current,
    "PATCH /feedback/5": (body) => {
      current = { ...current, status: (body as { status: FeedbackDetail["status"] }).status };
      return current;
    },
    "POST /feedback/5/notes": (body) => {
      current = {
        ...current,
        notes: [
          { id: 1, user_id: member.id, author: member.display_name, kind: "note", body: (body as { body: string }).body, created_at: new Date().toISOString() },
        ],
      };
      return current;
    },
  });
  renderApp("/inbox/5");

  expect(await screen.findByRole("heading", { name: /王小明/ })).toBeInTheDocument();
  expect(screen.getByText("budget")).toBeInTheDocument();
  // 失敗的通知要看得到原因
  expect(screen.getByText("HTTP 500")).toBeInTheDocument();
  expect(screen.queryByTitle("刪除訊息")).not.toBeInTheDocument();

  await userEvent.selectOptions(screen.getByLabelText("狀態"), "in_progress");
  await waitFor(() => expect(calls.find((c) => c.method === "PATCH")?.body).toEqual({ status: "in_progress" }));

  await userEvent.type(screen.getByLabelText("新增備註"), "  已經回電  ");
  await userEvent.click(screen.getByRole("button", { name: "新增備註" }));
  expect(await screen.findByText("已經回電")).toBeInTheDocument();
  expect(calls.find((c) => c.path === "/feedback/5/notes")?.body).toEqual({ body: "已經回電" });
  // 自己寫的備註可以刪
  expect(screen.getByTitle("刪除備註")).toBeInTheDocument();
});
