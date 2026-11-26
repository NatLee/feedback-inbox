import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import { expect, test } from "vitest";
import { ArrivalCalendar, SiteWorkload, TriageBoard } from "./TriageBoard";
import { admin, emptyStats, item, meta, site } from "../test/fixtures";
import { fakeApi } from "../test/fakeApi";
import { renderApp } from "../test/render";

const stats = {
  ...emptyStats,
  by_status: { new: 3, in_progress: 2, done: 5, spam: 8 },
  open_age: { fresh: 1, waiting: 1, overdue: 3 },
};
test("triage uses non-spam counts and links to the exact age queue", () => {
  render(
    <MemoryRouter>
      <TriageBoard data={stats} />
    </MemoryRouter>,
  );
  expect(screen.getByRole("img")).toHaveAccessibleName(
    "未處理 3、處理中 2、已完成 5 則",
  );
  expect(
    screen.getByRole("link", { name: "已滿 7 天：3 封，查看最舊來信" }),
  ).toHaveAttribute("href", "/inbox?status=open&age=overdue&sort=oldest");
});
test("zero arrival dates remain clickable and site workload can hide completed sites", async () => {
  render(
    <MemoryRouter>
      <ArrivalCalendar
        data={{
          ...stats,
          daily: [
            { date: "2026-03-01", count: 0 },
            { date: "2026-03-02", count: 8 },
          ],
        }}
      />
      <SiteWorkload
        sites={[
          {
            id: 1,
            name: "Finished",
            active: true,
            total: 8,
            unhandled: 0,
            last_at: null,
          },
          {
            id: 2,
            name: "Waiting",
            active: true,
            total: 5,
            unhandled: 3,
            last_at: null,
          },
        ]}
      />
    </MemoryRouter>,
  );
  expect(
    screen.getByRole("link", { name: "2026-03-01，0 封來信" }),
  ).toHaveAttribute("href", "/inbox?from=2026-03-01&to=2026-03-01");
  expect(screen.getByText("Finished")).toBeInTheDocument();
  await userEvent.click(screen.getByLabelText("只看待處理"));
  expect(screen.queryByText("Finished")).not.toBeInTheDocument();
  expect(screen.getByRole("link", { name: /Waiting/ })).toHaveAttribute(
    "href",
    "/inbox?site_id=2&status=open&sort=oldest",
  );
});
test("age drilldown keeps API and CSV filters and clear removes them", async () => {
  const queries: URLSearchParams[] = [];
  fakeApi({
    "GET /meta": () => meta,
    "GET /auth/state": () => ({ setup_required: false, user: admin }),
    "GET /sites": () => [site],
    "GET /feedback": (_body, url) => {
      queries.push(url.searchParams);
      return { items: [item(1)], total: 1, page: 1, per_page: 25 };
    },
  });
  renderApp("/inbox?status=open&age=overdue&sort=oldest");
  await screen.findByText("第 1 則訊息");
  expect(queries.at(-1)?.get("age")).toBe("overdue");
  expect(queries.at(-1)?.get("sort")).toBe("oldest");
  expect(screen.getByLabelText("收到多久")).toHaveValue("overdue");
  expect(screen.getByRole("link", { name: "匯出 CSV" })).toHaveAttribute(
    "href",
    expect.stringContaining("age=overdue&sort=oldest"),
  );
  await userEvent.selectOptions(screen.getByLabelText("收到多久"), "fresh");
  await waitFor(() => expect(queries.at(-1)?.get("age")).toBe("fresh"));
  await userEvent.click(screen.getByRole("button", { name: /清除/ }));
  await waitFor(() => expect(queries.at(-1)?.has("age")).toBe(false));
  expect(screen.getByLabelText("排序")).toHaveValue("newest");
});
