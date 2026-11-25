import { ArrowRight, CheckCircle2, CircleDot, Globe, Inbox, Loader, TrendingDown, TrendingUp, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { Link } from "react-router";
import { api } from "../api";
import { FeedbackRow } from "../components/feedback";
import { PageBody } from "../components/Layout";
import { Badge, Empty, ErrorBanner, Loading, PageHeader } from "../components/ui";
import { useUser } from "../context";
import { timeAgo } from "../lib/format";
import { useAsync } from "../lib/hooks";
import type { Stats } from "../types";

function StatCard({
  label,
  value,
  icon: Icon,
  to,
  children,
}: {
  label: string;
  value: number;
  icon: LucideIcon;
  to?: string;
  children?: ReactNode;
}) {
  const body = (
    <>
      <div className="flex items-center justify-between text-zinc-500 dark:text-zinc-400">
        <span className="text-xs font-medium">{label}</span>
        <Icon className="size-4" />
      </div>
      <p className="mt-2 text-2xl font-semibold tracking-tight tabular-nums">{value.toLocaleString()}</p>
      <p className="mt-1 min-h-4 text-xs text-zinc-500 dark:text-zinc-400">{children}</p>
    </>
  );
  return to ? (
    <Link to={to} className="card block p-4 transition-colors hover:border-brand-300 dark:hover:border-brand-700">
      {body}
    </Link>
  ) : (
    <div className="card p-4">{body}</div>
  );
}

/** 這 7 天和前 7 天比。 */
function Trend({ current, previous }: { current: number; previous: number }) {
  if (previous === 0) return <>{current === 0 ? "前 7 天也沒有訊息" : "前 7 天沒有訊息"}</>;
  const change = Math.round(((current - previous) / previous) * 100);
  if (change === 0) return <>和前 7 天一樣</>;
  const Icon = change > 0 ? TrendingUp : TrendingDown;
  return (
    <span className="inline-flex items-center gap-1">
      <Icon className="size-3.5" />比前 7 天{change > 0 ? "多" : "少"} {Math.abs(change)}%
    </span>
  );
}

/** 最近 30 天每天幾則的長條圖。 */
function DailyChart({ daily }: { daily: Stats["daily"] }) {
  const max = Math.max(1, ...daily.map((d) => d.count));
  const total = daily.reduce((sum, d) => sum + d.count, 0);
  const label = (date: string) => date.slice(5).replace("-", "/");
  return (
    <section className="card p-4 sm:p-5">
      <div className="flex items-baseline justify-between">
        <h2 className="text-sm font-semibold">最近 30 天</h2>
        <span className="text-xs text-zinc-500 dark:text-zinc-400">共 {total.toLocaleString()} 則（不含垃圾訊息）</span>
      </div>
      <div className="mt-4 flex h-36 items-end gap-[3px]" role="img" aria-label={`最近 30 天共 ${total} 則訊息`}>
        {daily.map((d) => (
          <div key={d.date} className="group relative flex h-full flex-1 items-end">
            <div
              className={`w-full rounded-t-sm transition-colors ${
                d.count === 0
                  ? "bg-zinc-200 dark:bg-zinc-800"
                  : "bg-brand-500 group-hover:bg-brand-600 dark:bg-brand-500 dark:group-hover:bg-brand-400"
              }`}
              style={{ height: d.count === 0 ? 2 : `${Math.max(6, (d.count / max) * 100)}%` }}
            />
            <span className="pointer-events-none absolute bottom-full left-1/2 z-10 mb-1 hidden -translate-x-1/2 rounded-md bg-zinc-900 px-1.5 py-0.5 text-[11px] whitespace-nowrap text-white group-hover:block dark:bg-white dark:text-zinc-900">
              {label(d.date)}・{d.count} 則
            </span>
          </div>
        ))}
      </div>
      <div className="mt-1.5 flex justify-between text-[11px] text-zinc-500 tabular-nums dark:text-zinc-400">
        <span>{label(daily[0]?.date ?? "")}</span>
        <span>{label(daily[Math.floor(daily.length / 2)]?.date ?? "")}</span>
        <span>今天</span>
      </div>
    </section>
  );
}

function SiteList({ sites }: { sites: Stats["sites"] }) {
  return (
    <section className="card">
      <div className="flex items-center justify-between border-b border-zinc-100 px-4 py-3 sm:px-5 dark:border-zinc-800">
        <h2 className="text-sm font-semibold">網站</h2>
        <Link to="/sites" className="inline-flex items-center gap-1 text-xs text-brand-700 hover:underline dark:text-brand-300">
          全部 <ArrowRight className="size-3" />
        </Link>
      </div>
      <ul className="divide-y divide-zinc-100 dark:divide-zinc-800">
        {sites.map((s) => (
          <li key={s.id}>
            <Link
              to={`/inbox?site_id=${s.id}&status=open`}
              className="flex items-center justify-between gap-3 px-4 py-3 transition-colors hover:bg-zinc-50 sm:px-5 dark:hover:bg-zinc-800/50"
            >
              <span className="min-w-0">
                <span className="flex items-center gap-2 text-sm font-medium">
                  <span className="truncate">{s.name}</span>
                  {!s.active && <Badge>已停收</Badge>}
                </span>
                <span className="text-xs text-zinc-500 dark:text-zinc-400">
                  共 {s.total.toLocaleString()} 則・{s.last_at ? `最新 ${timeAgo(s.last_at)}` : "還沒有訊息"}
                </span>
              </span>
              {s.unhandled > 0 ? (
                <Badge tone="amber">{s.unhandled} 則待處理</Badge>
              ) : (
                <Badge tone="green">都處理完了</Badge>
              )}
            </Link>
          </li>
        ))}
      </ul>
    </section>
  );
}

export function Dashboard() {
  const user = useUser();
  const { data, error } = useAsync(() => api.stats(), []);

  return (
    <PageBody>
      <PageHeader title={`${user.display_name}，你好`} subtitle="各網站表單送進來的訊息，和還沒處理的數量。" />
      {error && <ErrorBanner message={error} />}
      {!data && !error && <Loading />}
      {data && data.sites.length === 0 && (
        <div className="card">
          <Empty icon={Globe} title={user.role === "admin" ? "先新增一個網站" : "你還沒有負責的網站"}>
            {user.role === "admin" ? (
              <>
                每個網站會有一個表單網址，把表單的 <code className="font-mono">action</code> 指過去就能開始收訊息。
                <div className="mt-4">
                  <Link to="/sites" className="btn btn-primary">
                    <Globe className="size-4" /> 新增網站
                  </Link>
                </div>
              </>
            ) : (
              "請管理員把你加進網站的負責人，就能看到那個網站的訊息。"
            )}
          </Empty>
        </div>
      )}
      {data && data.sites.length > 0 && (
        <div className="flex flex-col gap-4">
          <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
            <StatCard label="未處理" value={data.by_status.new} icon={CircleDot} to="/inbox?status=new">
              還沒有人看過
            </StatCard>
            <StatCard label="處理中" value={data.by_status.in_progress} icon={Loader} to="/inbox?status=in_progress">
              有人在跟了
            </StatCard>
            <StatCard label="已完成" value={data.by_status.done} icon={CheckCircle2} to="/inbox?status=done">
              另有 {data.by_status.spam.toLocaleString()} 則垃圾訊息
            </StatCard>
            <StatCard label="最近 7 天" value={data.last_7_days} icon={Inbox}>
              <Trend current={data.last_7_days} previous={data.previous_7_days} />
            </StatCard>
          </div>
          <DailyChart daily={data.daily} />
          <div className="grid gap-4 lg:grid-cols-5">
            <section className="card lg:col-span-3">
              <div className="flex items-center justify-between border-b border-zinc-100 px-4 py-3 sm:px-5 dark:border-zinc-800">
                <h2 className="text-sm font-semibold">最新訊息</h2>
                <Link
                  to="/inbox"
                  className="inline-flex items-center gap-1 text-xs text-brand-700 hover:underline dark:text-brand-300"
                >
                  收件匣 <ArrowRight className="size-3" />
                </Link>
              </div>
              {data.recent.length === 0 ? (
                <Empty icon={Inbox} title="還沒有訊息">
                  表單一有人送出，就會出現在這裡。
                </Empty>
              ) : (
                <div className="divide-y divide-zinc-100 dark:divide-zinc-800">
                  {data.recent.map((item) => (
                    <FeedbackRow key={item.id} item={item} />
                  ))}
                </div>
              )}
            </section>
            <div className="lg:col-span-2">
              <SiteList sites={data.sites} />
            </div>
          </div>
        </div>
      )}
    </PageBody>
  );
}
