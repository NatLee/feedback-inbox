import { ArrowRight, CheckCheck, Clock3 } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";
import type { FeedbackAge, Stats } from "../types";

export const AGE_LANES: { key: FeedbackAge; title: string; hint: string }[] = [
  { key: "overdue", title: "已滿 7 天", hint: "先接住等得最久的聲音" },
  { key: "waiting", title: "1–7 天內", hint: "超過 24 小時，未滿 7 天" },
  { key: "fresh", title: "未滿 24 小時", hint: "新到的訊息，開始一段對話" },
];

export function TriageBoard({ data }: { data: Stats }) {
  const open = data.by_status.new + data.by_status.in_progress;
  const total = open + data.by_status.done;
  const statuses = [
    {
      key: "new",
      title: "未處理",
      value: data.by_status.new,
      color: "#c44f32",
    },
    {
      key: "in_progress",
      title: "處理中",
      value: data.by_status.in_progress,
      color: "#ce9b47",
    },
    {
      key: "done",
      title: "已完成",
      value: data.by_status.done,
      color: "#6d8a73",
    },
  ];
  let offset = 0;
  return (
    <section className="triage-board" aria-label="來信分流工作台">
      <div className="triage-seal">
        <span className="eyebrow">01 / THE OPEN LETTERS</span>
        <h2>{open ? "每一封等待，都值得回應。" : "今天的等待，已經接住。"}</h2>
        <div className="status-seal">
          <svg
            viewBox="0 0 200 200"
            role="img"
            aria-label={`未處理 ${data.by_status.new}、處理中 ${data.by_status.in_progress}、已完成 ${data.by_status.done} 則`}
          >
            <circle
              cx="100"
              cy="100"
              r="82"
              fill="none"
              stroke="var(--desk-rule)"
              strokeWidth="15"
            />
            {statuses.map((s) => {
              const part = total ? (s.value / total) * 100 : 0;
              const start = offset;
              offset += part;
              return (
                <circle
                  key={s.key}
                  cx="100"
                  cy="100"
                  r="82"
                  fill="none"
                  stroke={s.color}
                  strokeWidth="15"
                  pathLength="100"
                  strokeDasharray={`${part} ${100 - part}`}
                  strokeDashoffset={-start}
                  transform="rotate(-90 100 100)"
                />
              );
            })}
          </svg>
          <div>
            <strong>{open.toLocaleString()}</strong>
            <span>封等待處理</span>
            <small>共 {total.toLocaleString()} 封 · 不含垃圾</small>
          </div>
        </div>
        <div className="status-key">
          {statuses.map((s) => (
            <Link key={s.key} to={`/inbox?status=${s.key}`}>
              <i style={{ background: s.color }} />
              <span>{s.title}</span>
              <b>{s.value.toLocaleString()}</b>
              <ArrowRight size={13} />
            </Link>
          ))}
        </div>
      </div>
      <div className="waiting-ledger">
        <div className="insight-heading">
          <div>
            <span className="eyebrow">PRIORITY / 按等待時間</span>
            <h3>先回應，等最久的人。</h3>
          </div>
          <Clock3 size={22} />
        </div>
        <p className="insight-caption">
          只計未處理與處理中的訊息；從收到的時間開始計算。
        </p>
        <div className="age-lanes">
          {AGE_LANES.map((lane, i) => (
            <Link
              key={lane.key}
              className={`age-lane age-${lane.key}`}
              to={`/inbox?status=open&age=${lane.key}&sort=oldest`}
              aria-label={`${lane.title}：${data.open_age[lane.key]} 封，查看最舊來信`}
            >
              <span className="lane-number">0{i + 1}</span>
              <span className="lane-copy">
                <b>{lane.title}</b>
                <small>{lane.hint}</small>
                <span className="lane-track">
                  <span
                    style={{
                      width: `${open ? (data.open_age[lane.key] / open) * 100 : 0}%`,
                    }}
                  />
                </span>
              </span>
              <strong>{data.open_age[lane.key]}</strong>
              <ArrowRight size={17} />
            </Link>
          ))}
        </div>
        <div className="ledger-foot">
          {open ? (
            <>
              <span>選擇一列，從最舊來信開始。</span>
              <Link to="/inbox?status=open&sort=oldest">
                查看全部待處理 <ArrowRight size={14} />
              </Link>
            </>
          ) : (
            <>
              <CheckCheck size={18} />
              <span>沒有待處理的訊息。新來信會出現在這裡。</span>
            </>
          )}
        </div>
      </div>
    </section>
  );
}

export function ArrivalCalendar({ data }: { data: Stats }) {
  const max = Math.max(1, ...data.daily.map((d) => d.count));
  const total = data.daily.reduce((sum, d) => sum + d.count, 0);
  return (
    <section className="arrival-calendar card">
      <div className="insight-heading">
        <div>
          <span className="eyebrow">02 / ARRIVAL CALENDAR</span>
          <h3>來信的節奏</h3>
        </div>
        <span className="calendar-total">
          <b>{total.toLocaleString()}</b> 封 / 30 天
        </span>
      </div>
      <p className="insight-caption">
        依你的時區，點日期查看當天來信。不含垃圾訊息。
      </p>
      <div className="arrival-grid">
        {data.daily.map((day) => (
          <Link
            key={day.date}
            to={`/inbox?from=${day.date}&to=${day.date}`}
            className={`arrival-cell intensity-${day.count === 0 ? 0 : Math.ceil((day.count / max) * 4)}`}
            aria-label={`${day.date}，${day.count} 封來信`}
          >
            <span>{day.date.slice(5).replace("-", "/")}</span>
            <strong>{day.count}</strong>
          </Link>
        ))}
      </div>
      {total === 0 && (
        <p className="insight-caption">
          最近 30 天還沒有來信。確認網站表單已接上收件網址。
        </p>
      )}
      <div className="calendar-footer">
        <span>
          近 7 天 <b>{data.last_7_days}</b> 封 · 前 7 天 {data.previous_7_days}{" "}
          封
        </span>
        <span className="calendar-legend">
          少{" "}
          {[0, 1, 2, 3, 4].map((n) => (
            <i key={n} className={`intensity-${n}`} />
          ))}{" "}
          多
        </span>
      </div>
    </section>
  );
}

export function SiteWorkload({ sites }: { sites: Stats["sites"] }) {
  const [onlyOpen, setOnlyOpen] = useState(false);
  const sorted = [...sites]
    .filter((s) => !onlyOpen || s.unhandled > 0)
    .sort(
      (a, b) =>
        b.unhandled - a.unhandled ||
        b.total - a.total ||
        a.name.localeCompare(b.name),
    );
  const max = Math.max(1, ...sites.map((s) => s.total));
  return (
    <section className="site-workload card">
      <div className="insight-heading">
        <div>
          <span className="eyebrow">03 / ACROSS THE DESK</span>
          <h3>網站處理負載</h3>
        </div>
        <label className="workload-filter">
          <input
            type="checkbox"
            checked={onlyOpen}
            onChange={(e) => setOnlyOpen(e.target.checked)}
          />{" "}
          只看待處理
        </label>
      </div>
      <p className="insight-caption">
        依待處理封數排序，點網站進入它的待處理信件。
      </p>
      <div className="workload-legend">
        <span>
          <i />
          待處理
        </span>
        <span>
          <i />
          已完成
        </span>
      </div>
      <div className="workload-rows">
        {sorted.map((site, i) => (
          <Link
            key={site.id}
            to={`/inbox?site_id=${site.id}&status=open&sort=oldest`}
            className="workload-row"
          >
            <span className="workload-rank">
              {String(i + 1).padStart(2, "0")}
            </span>
            <div>
              <div className="workload-name">
                <b>{site.name}</b>
                <span>
                  {site.unhandled} 待處理 / {site.total} 封
                </span>
              </div>
              <span className="workload-bar">
                <i style={{ width: `${(site.unhandled / max) * 100}%` }} />
                <i
                  style={{
                    width: `${((site.total - site.unhandled) / max) * 100}%`,
                  }}
                />
              </span>
            </div>
            <ArrowRight size={16} />
          </Link>
        ))}
      </div>
      {sorted.length === 0 && (
        <p className="workload-empty">
          各網站都沒有待處理訊息。取消勾選可查看全部網站。
        </p>
      )}
      <Link to="/sites" className="workload-manage">
        管理網站與收件設定 <ArrowRight size={14} />
      </Link>
    </section>
  );
}
