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
    { key: "new", title: "未處理", value: data.by_status.new },
    { key: "in_progress", title: "處理中", value: data.by_status.in_progress },
    { key: "done", title: "已完成", value: data.by_status.done },
  ];
  return (
    <section className="correspondence-desk" aria-label="來信分流工作台">
      <div className="desk-editorial">
        <div className="desk-edition">
          <span>THE CORRESPONDENCE DESK</span>
          <span>來信處理編輯台 / 01</span>
        </div>
        <div className="editorial-copy">
          <div>
            <span className="eyebrow">LET EVERY VOICE BE HEARD</span>
            <h2>
              {open ? (
                <>
                  把等待，
                  <br />
                  變成回應。
                </>
              ) : (
                <>
                  每一封，
                  <br />
                  都有了回音。
                </>
              )}
            </h2>
            <p>
              先接住等得最久的聲音。
              <br />
              從一封來信，開始今天的工作。
            </p>
          </div>
          <Link to="/inbox?status=open&sort=oldest" className="open-postmark">
            <span>WAITING TO BE HEARD</span>
            <strong>{open.toLocaleString()}</strong>
            <span>
              封等待處理 <ArrowRight size={16} />
            </span>
          </Link>
        </div>
        <div
          className="mail-status-register"
          role="group"
          aria-label={`未處理 ${data.by_status.new}、處理中 ${data.by_status.in_progress}、已完成 ${data.by_status.done} 則`}
        >
          {statuses.map((s, i) => (
            <Link key={s.key} to={`/inbox?status=${s.key}`}>
              <small>
                0{i + 1} / {s.title}
              </small>
              <strong>
                {s.value.toLocaleString()}
                <ArrowRight size={16} />
              </strong>
            </Link>
          ))}
        </div>
        <span className="register-foot">
          收件登錄 {total.toLocaleString()} 封 · 不含垃圾訊息
        </span>
      </div>
      <div className="letter-sorting">
        <div className="sorting-heading">
          <span className="eyebrow">SORTED BY RECEIVED AGE</span>
          <Clock3 size={16} />
        </div>
        <h3>今天，先拆哪一封？</h3>
        <p>收到至今的時間 · 僅待處理來信</p>
        <div className="envelope-trays">
          {AGE_LANES.map((lane, i) => (
            <Link
              key={lane.key}
              className={`sorting-envelope envelope-${lane.key}`}
              to={`/inbox?status=open&age=${lane.key}&sort=oldest`}
              aria-label={`${lane.title}：${data.open_age[lane.key]} 封，查看最舊來信`}
            >
              <div className="envelope-letter">
                <span>TO: THE TEAM</span>
                <b>{lane.title}</b>
                <small>{lane.hint}</small>
              </div>
              <div className="envelope-front">
                <span className="envelope-index">
                  0{i + 1}
                  <small>RECEIVED</small>
                </span>
                <strong>
                  {data.open_age[lane.key]}
                  <small>封</small>
                </strong>
                <span className="envelope-open">
                  拆閱 <ArrowRight size={16} />
                </span>
              </div>
            </Link>
          ))}
        </div>
        <div className="sorting-caption">
          {open ? (
            <>
              <span>依收件時間分袋，從最舊開始閱讀。</span>
              <Link to="/inbox?status=open&sort=oldest">
                全部待處理 <ArrowRight size={16} />
              </Link>
            </>
          ) : (
            <>
              <CheckCheck size={16} />
              <span>沒有待處理訊息。新來信會出現在這裡。</span>
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
        管理網站與收件設定 <ArrowRight size={16} />
      </Link>
    </section>
  );
}
