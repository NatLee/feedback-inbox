import {
  ChevronLeft,
  ChevronRight,
  Download,
  Inbox,
  MessageSquare,
  Search,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { api } from "../api";
import { StatusBadge, senderName } from "../components/feedback";
import { PageBody } from "../components/Layout";
import {
  Empty,
  ErrorBanner,
  Loading,
  PageHeader,
  Spinner,
  useToast,
} from "../components/ui";
import { STATUSES, STATUS_LABEL, timeAgo, dateTime } from "../lib/format";
import { errorText, useAsync, useDebounced } from "../lib/hooks";
import type {
  FeedbackAge,
  FeedbackFilter,
  Status,
  StatusFilter,
} from "../types";

const PER_PAGE = 25;

const TABS: { value: StatusFilter | ""; label: string }[] = [
  { value: "", label: "全部" },
  { value: "open", label: "待處理" },
  { value: "new", label: STATUS_LABEL.new },
  { value: "in_progress", label: STATUS_LABEL.in_progress },
  { value: "done", label: STATUS_LABEL.done },
  { value: "spam", label: STATUS_LABEL.spam },
];

/** 日期欄位（當地的 YYYY-MM-DD）換成 API 要的時間；`addDays` 用來把結束日算到隔天零點。 */
function localMidnight(date: string, addDays = 0): string | undefined {
  const [y, m, d] = date.split("-").map(Number);
  if (!y || !m || !d) return undefined;
  return new Date(y, m - 1, d + addDays).toISOString();
}

export function InboxPage() {
  const toast = useToast();
  const [params, setParams] = useSearchParams();
  const status = (params.get("status") ?? "") as StatusFilter | "";
  const siteId = Number(params.get("site_id")) || undefined;
  const age = (params.get("age") || undefined) as FeedbackAge | undefined;
  const sort = (params.get("sort") || "newest") as "newest" | "oldest";
  const from = params.get("from") ?? "";
  const to = params.get("to") ?? "";
  const page = Math.max(1, Number(params.get("page")) || 1);

  // 搜尋框自己記著正在打的字，停一下才寫進網址
  const [search, setSearch] = useState(params.get("q") ?? "");
  const q = useDebounced(search.trim(), 300);

  /** 改篩選條件：寫進網址（重新整理、分享連結都還在），並回到第一頁。 */
  const set = (changes: Record<string, string | undefined>) =>
    setParams(
      (current) => {
        const next = new URLSearchParams(current);
        if (!("page" in changes)) next.delete("page");
        for (const [key, value] of Object.entries(changes)) {
          if (value) next.set(key, value);
          else next.delete(key);
        }
        return next;
      },
      { replace: true },
    );

  useEffect(() => {
    if (q !== (params.get("q") ?? "")) set({ q: q || undefined });
    // 只在打完字的時候同步
  }, [q]);

  const filter: FeedbackFilter = {
    age,
    sort: sort === "newest" ? undefined : sort,
    site_id: siteId,
    status: status || undefined,
    q: params.get("q") ?? undefined,
    from: localMidnight(from),
    to: localMidnight(to, 1),
  };
  const filterKey = JSON.stringify(filter);

  const sites = useAsync(() => api.sites(), []);
  const list = useAsync(
    () => api.feedback({ ...filter, page, per_page: PER_PAGE }),
    [filterKey, page],
  );

  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  // 換了條件或頁數，之前勾的就不算了
  useEffect(() => setSelected(new Set()), [filterKey, page]);

  const items = list.data?.items ?? [];
  const total = list.data?.total ?? 0;
  const pages = Math.max(1, Math.ceil(total / PER_PAGE));
  const allSelected =
    items.length > 0 && items.every((i) => selected.has(i.id));
  const filtered = Boolean(
    status || siteId || filter.q || from || to || age || sort !== "newest",
  );

  const toggle = (id: number) =>
    setSelected((current) => {
      const next = new Set(current);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  const bulk = async (next: Status) => {
    setBusy(true);
    try {
      const { updated } = await api.bulkStatus([...selected], next);
      toast(`${updated} 則改成「${STATUS_LABEL[next]}」`);
      setSelected(new Set());
      await list.reload();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <PageBody>
      <PageHeader
        title="收件匣"
        subtitle="INCOMING MAIL / 依來源與收件時間，整理每一個聲音。"
        actions={
          <a
            className="btn btn-secondary"
            href={api.exportUrl(filter)}
            download
          >
            <Download className="size-4" /> 匯出 CSV
          </a>
        }
      />

      <div
        className="mail-folders mb-3 flex flex-wrap gap-1"
        role="tablist"
        aria-label="狀態"
      >
        {TABS.map((tab) => (
          <button
            key={tab.value}
            role="tab"
            aria-selected={status === tab.value}
            onClick={() => set({ status: tab.value || undefined })}
            className={`rounded-lg px-3 py-1.5 text-sm transition-colors ${
              status === tab.value
                ? "bg-brand-600 font-medium text-white dark:bg-brand-500 dark:text-zinc-950"
                : "text-zinc-600 hover:bg-zinc-200/70 dark:text-zinc-400 dark:hover:bg-zinc-800"
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>

      <div className="mail-tools card mb-4 flex flex-wrap items-center gap-2 p-3">
        <div className="relative min-w-48 flex-1">
          <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-zinc-400" />
          <input
            className="input pl-9"
            type="search"
            aria-label="搜尋"
            placeholder="搜尋姓名、信箱、電話、公司或訊息內容"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <select
          className="input w-auto"
          aria-label="網站"
          value={siteId ?? ""}
          onChange={(e) => set({ site_id: e.target.value || undefined })}
        >
          <option value="">所有網站</option>
          {sites.data?.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </select>
        <div className="flex items-center gap-1.5 text-sm text-zinc-500 dark:text-zinc-400">
          <input
            className="input w-auto"
            type="date"
            aria-label="從哪一天"
            value={from}
            max={to || undefined}
            onChange={(e) => set({ from: e.target.value || undefined })}
          />
          到
          <input
            className="input w-auto"
            type="date"
            aria-label="到哪一天"
            value={to}
            min={from || undefined}
            onChange={(e) => set({ to: e.target.value || undefined })}
          />
        </div>
        <select
          className="input w-auto"
          aria-label="收到多久"
          value={age ?? ""}
          onChange={(e) => set({ age: e.target.value || undefined })}
        >
          <option value="">不限收件時間</option>
          <option value="fresh">未滿 24 小時</option>
          <option value="waiting">1–7 天內</option>
          <option value="overdue">已滿 7 天</option>
        </select>
        <select
          className="input w-auto"
          aria-label="排序"
          value={sort}
          onChange={(e) =>
            set({
              sort: e.target.value === "newest" ? undefined : e.target.value,
            })
          }
        >
          <option value="newest">最新來信優先</option>
          <option value="oldest">最舊來信優先</option>
        </select>
        {filtered && (
          <button
            className="btn btn-ghost"
            onClick={() => {
              setSearch("");
              setParams({}, { replace: true });
            }}
          >
            <X className="size-4" /> 清除
          </button>
        )}
      </div>

      {list.error && <ErrorBanner message={list.error} />}
      {!list.data && !list.error && <Loading />}
      {list.data && (
        <div className="mail-list card overflow-hidden">
          <div className="flex min-h-12 flex-wrap items-center gap-2 border-b border-zinc-100 px-3 py-2 dark:border-zinc-800">
            <label className="flex items-center gap-2.5 text-sm">
              <input
                type="checkbox"
                className="size-4 accent-brand-600"
                aria-label="全選這一頁"
                checked={allSelected}
                disabled={items.length === 0}
                onChange={() =>
                  setSelected(
                    allSelected ? new Set() : new Set(items.map((i) => i.id)),
                  )
                }
              />
              {selected.size > 0 ? (
                <span className="font-medium">選了 {selected.size} 則</span>
              ) : (
                <span className="text-zinc-500 dark:text-zinc-400">
                  共 {total.toLocaleString()} 則
                </span>
              )}
            </label>
            {selected.size > 0 && (
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="text-xs text-zinc-500 dark:text-zinc-400">
                  改成
                </span>
                {STATUSES.map((s) => (
                  <button
                    key={s}
                    className="btn btn-secondary px-2 py-1 text-xs"
                    disabled={busy}
                    onClick={() => void bulk(s)}
                  >
                    {STATUS_LABEL[s]}
                  </button>
                ))}
                {busy && <Spinner />}
              </div>
            )}
          </div>

          {items.length === 0 ? (
            <Empty
              icon={Inbox}
              title={filtered ? "沒有符合條件的訊息" : "還沒有訊息"}
            >
              {filtered
                ? "換個條件試試，或按「清除」看全部。"
                : "表單一有人送出，就會出現在這裡。"}
            </Empty>
          ) : (
            <ul className="mail-register divide-y divide-zinc-100 dark:divide-zinc-800">
              {items.map((item) => (
                <li
                  key={item.id}
                  className={`mail-row flex items-start gap-3 px-3 py-3 transition-colors hover:bg-zinc-50 dark:hover:bg-zinc-800/50 ${
                    selected.has(item.id)
                      ? "bg-brand-50/60 dark:bg-brand-950/30"
                      : ""
                  }`}
                >
                  <input
                    type="checkbox"
                    className="mt-1 size-4 shrink-0 accent-brand-600"
                    aria-label={`選取 ${senderName(item)} 的訊息`}
                    checked={selected.has(item.id)}
                    onChange={() => toggle(item.id)}
                  />
                  <span className="sender-seal" aria-hidden="true">
                    {Array.from(senderName(item))[0]}
                  </span>
                  <Link
                    to={`/inbox/${item.id}`}
                    className="grid min-w-0 flex-1 gap-x-4 gap-y-1 sm:grid-cols-[13rem_1fr_auto]"
                  >
                    <span className="min-w-0">
                      <span
                        className={`block truncate text-sm ${item.status === "new" ? "font-semibold" : "font-medium"}`}
                      >
                        {item.status === "new" && (
                          <span
                            className="mr-1.5 inline-block size-2 rounded-full bg-amber-500 align-middle"
                            aria-hidden
                          />
                        )}
                        {senderName(item)}
                      </span>
                      <span className="block truncate text-xs text-zinc-500 dark:text-zinc-400">
                        {item.company || item.email || "—"}
                      </span>
                    </span>
                    <span className="min-w-0">
                      <span className="block truncate text-sm text-zinc-700 dark:text-zinc-300">
                        {item.preview || "（沒有內容）"}
                      </span>
                      <span className="flex items-center gap-2 text-xs text-zinc-500 dark:text-zinc-400">
                        <span className="mail-routing">{item.site_name}</span>
                        {item.note_count > 0 && (
                          <span
                            className="inline-flex items-center gap-0.5"
                            title={`${item.note_count} 則備註`}
                          >
                            <MessageSquare className="size-3" />{" "}
                            {item.note_count}
                          </span>
                        )}
                      </span>
                    </span>
                    <span className="flex items-center gap-2 sm:flex-col sm:items-end sm:gap-1">
                      <StatusBadge status={item.status} />
                      <span
                        className="text-xs whitespace-nowrap text-zinc-500 dark:text-zinc-400"
                        title={dateTime(item.created_at)}
                      >
                        {timeAgo(item.created_at)}
                      </span>
                    </span>
                  </Link>
                </li>
              ))}
            </ul>
          )}

          {pages > 1 && (
            <div className="flex items-center justify-between border-t border-zinc-100 px-3 py-2 text-sm dark:border-zinc-800">
              <span className="text-zinc-500 tabular-nums dark:text-zinc-400">
                第 {page} / {pages} 頁
              </span>
              <span className="flex gap-1">
                <button
                  className="btn btn-secondary px-2"
                  disabled={page <= 1}
                  onClick={() => set({ page: String(page - 1) })}
                >
                  <ChevronLeft className="size-4" /> 上一頁
                </button>
                <button
                  className="btn btn-secondary px-2"
                  disabled={page >= pages}
                  onClick={() => set({ page: String(page + 1) })}
                >
                  下一頁 <ChevronRight className="size-4" />
                </button>
              </span>
            </div>
          )}
        </div>
      )}
    </PageBody>
  );
}
