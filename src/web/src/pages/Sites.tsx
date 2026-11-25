import { Bell, Globe, Plus, Users, Webhook } from "lucide-react";
import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router";
import { api } from "../api";
import { PageBody } from "../components/Layout";
import { Badge, Dialog, Empty, ErrorBanner, Loading, PageHeader, Spinner } from "../components/ui";
import { useUser } from "../context";
import { errorText, useAsync } from "../lib/hooks";

function NewSiteDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const navigate = useNavigate();
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setName("");
    setUrl("");
    setError(null);
  }, [open]);

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const site = await api.createSite({ name: name.trim(), url: url.trim() });
      // 建好就去設定頁：那裡有表單網址和嵌入的寫法
      navigate(`/sites/${site.id}`);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      title="新增網站"
      onClose={onClose}
      footer={
        <>
          <button className="btn btn-secondary" onClick={onClose}>
            取消
          </button>
          <button className="btn btn-primary" disabled={busy || !name.trim()} onClick={() => void save()}>
            {busy && <Spinner />} 建立
          </button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <div>
          <label className="label" htmlFor="site-name">
            名稱
          </label>
          <input id="site-name" className="input" maxLength={60} value={name} onChange={(e) => setName(e.target.value)} />
          <p className="hint">收件匣和通知裡用這個名字分辨訊息是哪裡來的。</p>
        </div>
        <div>
          <label className="label" htmlFor="site-url">
            網址
          </label>
          <input
            id="site-url"
            className="input"
            type="url"
            placeholder="https://www.example.com（可以不填）"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
          />
        </div>
        {error && <ErrorBanner message={error} />}
      </div>
    </Dialog>
  );
}

export function Sites() {
  const admin = useUser().role === "admin";
  const { data, error } = useAsync(() => api.sites(), []);
  const [creating, setCreating] = useState(false);

  return (
    <PageBody>
      <PageHeader
        title="網站"
        subtitle={admin ? "每個網站有自己的表單網址、負責人和通知方式。" : "你負責的網站。"}
        actions={
          admin && (
            <button className="btn btn-primary" onClick={() => setCreating(true)}>
              <Plus className="size-4" /> 新增網站
            </button>
          )
        }
      />
      {error && <ErrorBanner message={error} />}
      {!data && !error && <Loading />}
      {data && data.length === 0 && (
        <div className="card">
          <Empty icon={Globe} title={admin ? "還沒有網站" : "你還沒有負責的網站"}>
            {admin
              ? "新增一個網站，拿到表單網址之後貼進網頁的表單，就能開始收訊息。"
              : "請管理員把你加進網站的負責人。"}
          </Empty>
        </div>
      )}
      {data && data.length > 0 && (
        <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
          {data.map((site) => (
            <Link
              key={site.id}
              to={`/sites/${site.id}`}
              className="card flex flex-col gap-3 p-4 transition-colors hover:border-brand-300 dark:hover:border-brand-700"
            >
              <div className="flex items-start justify-between gap-2">
                <div className="min-w-0">
                  <h2 className="truncate font-semibold">{site.name}</h2>
                  <p className="truncate text-xs text-zinc-500 dark:text-zinc-400">{site.url || "沒有填網址"}</p>
                </div>
                {site.active ? <Badge tone="green">收件中</Badge> : <Badge>已停收</Badge>}
              </div>
              <div className="flex items-baseline gap-4">
                <p>
                  <span className="text-2xl font-semibold tracking-tight tabular-nums">{site.unhandled.toLocaleString()}</span>
                  <span className="ml-1 text-xs text-zinc-500 dark:text-zinc-400">待處理</span>
                </p>
                <p className="text-xs text-zinc-500 dark:text-zinc-400">共 {site.total.toLocaleString()} 則</p>
              </div>
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-t border-zinc-100 pt-3 text-xs text-zinc-500 dark:border-zinc-800 dark:text-zinc-400">
                <span className="inline-flex items-center gap-1">
                  <Users className="size-3.5" />
                  {site.members.length === 0 ? "沒有負責人" : site.members.map((m) => m.display_name).join("、")}
                </span>
                {site.notify_email && (
                  <span className="inline-flex items-center gap-1">
                    <Bell className="size-3.5" /> 通知信
                  </span>
                )}
                {site.has_webhook && (
                  <span className="inline-flex items-center gap-1">
                    <Webhook className="size-3.5" /> Webhook
                  </span>
                )}
              </div>
            </Link>
          ))}
        </div>
      )}
      <NewSiteDialog open={creating} onClose={() => setCreating(false)} />
    </PageBody>
  );
}
