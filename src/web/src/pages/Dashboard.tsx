import { ArrowRight, Globe, Inbox } from "lucide-react";
import { Link } from "react-router";
import { api } from "../api";
import { FeedbackRow } from "../components/feedback";
import { PageBody } from "../components/Layout";
import {
  ArrivalCalendar,
  SiteWorkload,
  TriageBoard,
} from "../components/TriageBoard";
import { Empty, ErrorBanner, PageHeader } from "../components/ui";
import { useUser } from "../context";
import { useAsync } from "../lib/hooks";

export function Dashboard() {
  const user = useUser();
  const { data, error, reload } = useAsync(() => api.stats(), []);
  return (
    <PageBody>
      <PageHeader
        title={`${user.display_name}，你好`}
        subtitle="CORRESPONDENCE / 收件、分流、留下處理進度"
        actions={
          <Link to="/inbox" className="btn btn-secondary">
            打開收件匣 <ArrowRight size={16} />
          </Link>
        }
      />
      {error && (
        <>
          <ErrorBanner message={error} />
          <button
            className="btn btn-secondary mb-4"
            onClick={() => void reload()}
          >
            重新載入工作台
          </button>
        </>
      )}
      {!data && !error && (
        <div role="status" className="insight-loading">
          <span>正在整理來信與等待時間…</span>
          <div />
          <div />
          <div />
        </div>
      )}
      {data && data.sites.length === 0 && (
        <div className="card">
          <Empty
            icon={Globe}
            title={
              user.role === "admin" ? "先新增一個網站" : "你還沒有負責的網站"
            }
          >
            {user.role === "admin" ? (
              <>
                <p>建立網站，取得表單收件網址，就能在這裡追蹤每一封來信。</p>
                <Link to="/sites" className="btn btn-primary mt-4">
                  新增網站 <ArrowRight size={16} />
                </Link>
              </>
            ) : (
              "請管理員將你加入網站的負責人。"
            )}
          </Empty>
        </div>
      )}
      {data && data.sites.length > 0 && (
        <div className="triage-layout">
          <TriageBoard data={data} />
          <div className="triage-secondary">
            <ArrivalCalendar data={data} />
            <SiteWorkload sites={data.sites} />
          </div>
          <section className="card">
            <div className="insight-heading recent-heading">
              <div>
                <span className="eyebrow">THE LATEST / 新到的信</span>
                <h3>最近來信</h3>
              </div>
              <Link to="/inbox" className="btn btn-ghost">
                全部來信 <ArrowRight size={16} />
              </Link>
            </div>
            {data.recent.length ? (
              <div className="divide-y divide-zinc-100 dark:divide-zinc-800">
                {data.recent.map((item) => (
                  <FeedbackRow key={item.id} item={item} />
                ))}
              </div>
            ) : (
              <Empty icon={Inbox} title="第一封來信，還在路上">
                表單有人送出後，這裡會顯示來信。可先到網站設定確認收件網址與必填欄位。
              </Empty>
            )}
          </section>
        </div>
      )}
    </PageBody>
  );
}
