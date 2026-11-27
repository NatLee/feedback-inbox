import { ArrowLeft, BellRing, Check, CheckCircle2, Copy, ExternalLink, Inbox, RefreshCw, Trash2, XCircle } from "lucide-react";
import { useState, type ReactNode } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api } from "../api";
import { PageBody } from "../components/Layout";
import { Badge, ErrorBanner, Loading, PageHeader, Spinner, useToast } from "../components/ui";
import { useApp, useUser } from "../context";
import { FIELDS, FIELD_LABEL, parseLines } from "../lib/format";
import { errorText, useAsync } from "../lib/hooks";
import type { Account, Field, Site, TestResult } from "../types";

function Section({ title, hint, actions, children }: { title: string; hint?: ReactNode; actions?: ReactNode; children: ReactNode }) {
  return (
    <section className="card">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-zinc-100 px-4 py-3 sm:px-5 dark:border-zinc-800">
        <div>
          <h2 className="text-sm font-semibold">{title}</h2>
          {hint && <p className="text-xs text-zinc-500 dark:text-zinc-400">{hint}</p>}
        </div>
        {actions}
      </div>
      {children}
    </section>
  );
}

function CopyButton({ text, label = "複製" }: { text: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  const toast = useToast();
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      toast("瀏覽器不讓我們複製，請自己選取文字", "error");
    }
  };
  return (
    <button type="button" className="btn btn-secondary px-2 py-1 text-xs" onClick={() => void copy()}>
      {copied ? <Check className="size-4" /> : <Copy className="size-4" />} {copied ? "複製了" : label}
    </button>
  );
}

function Snippet({ title, code }: { title: string; code: string }) {
  return (
    <div>
      <div className="mb-2 flex items-center justify-between">
        <h3 className="text-xs font-semibold text-zinc-600 dark:text-zinc-300">{title}</h3>
        <CopyButton text={code} />
      </div>
      <pre className="overflow-x-auto rounded-lg bg-zinc-900 p-3 font-mono text-xs leading-5 text-zinc-100 dark:bg-zinc-950 dark:ring-1 dark:ring-zinc-800">
        {code}
      </pre>
    </div>
  );
}

function htmlSnippet(endpoint: string, required: Field[]): string {
  const req = (f: Field) => (required.includes(f) ? " required" : "");
  return `<form action="${endpoint}" method="post">
  <input name="name" placeholder="姓名"${req("name")}>
  <input name="email" type="email" placeholder="電子信箱"${req("email")}>
  <textarea name="message" placeholder="想說的話"${req("message")}></textarea>
  <!-- 人看不到、機器人會填的欄位：有填就當成垃圾訊息 -->
  <input name="_gotcha" style="display:none" tabindex="-1" autocomplete="off">
  <button>送出</button>
</form>`;
}

function fetchSnippet(endpoint: string): string {
  return `const res = await fetch("${endpoint}", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ name, email, message }),
});
if (!res.ok) alert((await res.json()).error);`;
}

/** 表單網址和貼到網頁上的寫法。 */
function Embed({ site, onRotated }: { site: Site; onRotated: (site: Site) => void }) {
  const admin = useUser().role === "admin";
  const { meta } = useApp();
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const endpoint = `${meta?.public_url || window.location.origin}/f/${site.key}`;

  const rotate = async () => {
    if (!window.confirm("換一個新的表單網址？舊的網址馬上失效，網頁上的表單要跟著改。")) return;
    setBusy(true);
    try {
      onRotated(await api.rotateKey(site.id));
      toast("換好了，記得更新網頁上的表單");
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section title="表單網址" hint="把表單送到這個網址，訊息就會進收件匣。">
      <div className="flex flex-col gap-4 p-4 sm:p-5">
        <div className="flex flex-wrap items-center gap-2">
          <code className="min-w-0 flex-1 truncate rounded-lg bg-zinc-100 px-3 py-2 font-mono text-sm dark:bg-zinc-800">{endpoint}</code>
          <CopyButton text={endpoint} />
          <a className="btn btn-secondary px-2 py-1 text-xs" href={endpoint} target="_blank" rel="noreferrer noopener">
            <ExternalLink className="size-4" /> 現成的表單頁
          </a>
          {admin && (
            <button className="btn btn-secondary px-2 py-1 text-xs" disabled={busy} onClick={() => void rotate()}>
              {busy ? <Spinner className="size-3.5" /> : <RefreshCw className="size-4" />} 換網址
            </button>
          )}
        </div>
        <Snippet title="HTML 表單" code={htmlSnippet(endpoint, site.required_fields)} />
        <Snippet title="用 JavaScript 送（JSON）" code={fetchSnippet(endpoint)} />
        <p className="text-xs leading-5 text-zinc-500 dark:text-zinc-400">
          認得的欄位是 <code className="font-mono">name</code>、<code className="font-mono">email</code>、
          <code className="font-mono">phone</code>、<code className="font-mono">company</code>、
          <code className="font-mono">message</code>；其他欄位會原樣留在訊息裡。還沒有自己的網頁的話，「現成的表單頁」可以直接把連結給人填。
        </p>
      </div>
    </Section>
  );
}

function TestOutcome({ result }: { result: TestResult }) {
  return (
    <div className="mt-3 rounded-lg bg-zinc-50 p-3 text-sm dark:bg-zinc-800/60">
      {result.outcomes.length === 0 && (
        <p className="text-zinc-600 dark:text-zinc-300">
          {result.no_recipients
            ? "沒有寄出：負責人都還沒填電子信箱（到「設定」裡填）。"
            : !result.smtp_configured
              ? "沒有東西可以送：伺服器沒有設定寄信（FI_SMTP_HOST），也沒有填 Webhook。"
              : "沒有東西可以送：通知信和 Webhook 都沒開。"}
        </p>
      )}
      <ul className="flex flex-col gap-2">
        {result.outcomes.map((o, i) => (
          <li key={i} className="flex items-start gap-2">
            {o.ok ? (
              <CheckCircle2 className="mt-1 size-4 shrink-0 text-emerald-600 dark:text-emerald-400" />
            ) : (
              <XCircle className="mt-1 size-4 shrink-0 text-rose-600 dark:text-rose-400" />
            )}
            <span className="min-w-0">
              <span className="break-all">
                {o.channel === "email" ? "通知信" : "Webhook"} → {o.target}
              </span>
              {!o.ok && <span className="block text-xs break-words text-rose-600 dark:text-rose-400">{o.detail}</span>}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** 管理員改網站設定的表單。 */
function SettingsForm({ site, accounts, onSaved }: { site: Site; accounts: Account[]; onSaved: (site: Site) => void }) {
  const { meta } = useApp();
  const toast = useToast();
  const navigate = useNavigate();
  const [name, setName] = useState(site.name);
  const [url, setUrl] = useState(site.url);
  const [active, setActive] = useState(site.active);
  const [required, setRequired] = useState<Set<Field>>(new Set(site.required_fields));
  const [origins, setOrigins] = useState(site.allowed_origins.join("\n"));
  const [redirectUrl, setRedirectUrl] = useState(site.redirect_url);
  const [notifyEmail, setNotifyEmail] = useState(site.notify_email);
  const [webhookUrl, setWebhookUrl] = useState(site.webhook_url);
  const [members, setMembers] = useState<Set<number>>(new Set(site.members.map((m) => m.id)));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [test, setTest] = useState<TestResult | null>(null);

  const toggle = <T,>(set: Set<T>, value: T): Set<T> => {
    const next = new Set(set);
    if (!next.delete(value)) next.add(value);
    return next;
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      onSaved(
        await api.updateSite(site.id, {
          name: name.trim(),
          url: url.trim(),
          active,
          required_fields: FIELDS.filter((f) => required.has(f)),
          allowed_origins: parseLines(origins),
          redirect_url: redirectUrl.trim(),
          notify_email: notifyEmail,
          webhook_url: webhookUrl.trim(),
          member_ids: [...members],
        }),
      );
      toast("設定存好了");
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const sendTest = async () => {
    setTesting(true);
    try {
      setTest(await api.testNotification(site.id));
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setTesting(false);
    }
  };

  const remove = async () => {
    const typed = window.prompt(`刪除網站會連同它的 ${site.total} 則訊息一起刪掉，不能復原。\n要刪的話請打網站名稱「${site.name}」：`);
    if (typed === null) return;
    if (typed.trim() !== site.name) {
      toast("名稱不一樣，沒有刪除", "error");
      return;
    }
    try {
      await api.deleteSite(site.id);
      toast(`刪除了「${site.name}」`);
      navigate("/sites");
    } catch (e) {
      toast(errorText(e), "error");
    }
  };

  const selectedWithoutEmail = accounts.filter((a) => members.has(a.id) && !a.email);

  return (
    <form
      className="flex flex-col gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <Section title="基本設定">
        <div className="grid gap-4 p-4 sm:grid-cols-2 sm:p-5">
          <div>
            <label className="label" htmlFor="site-name">
              名稱
            </label>
            <input id="site-name" className="input" maxLength={60} value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div>
            <label className="label" htmlFor="site-url">
              網址
            </label>
            <input
              id="site-url"
              className="input"
              type="url"
              placeholder="https://www.example.com"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
            />
          </div>
          <fieldset className="sm:col-span-2">
            <legend className="label">必填欄位</legend>
            <div className="flex flex-wrap gap-x-5 gap-y-2">
              {FIELDS.map((f) => (
                <label key={f} className="flex cursor-pointer items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    className="size-4 accent-brand-600"
                    checked={required.has(f)}
                    onChange={() => setRequired((s) => toggle(s, f))}
                  />
                  {FIELD_LABEL[f]}
                </label>
              ))}
            </div>
            <p className="hint">少了必填欄位的表單會被退回，不會進收件匣。</p>
          </fieldset>
          <div className="sm:col-span-2">
            <label className="label" htmlFor="site-origins">
              允許的來源
            </label>
            <textarea
              id="site-origins"
              className="input min-h-20 font-mono text-xs"
              placeholder={"https://www.example.com\nhttps://example.com"}
              value={origins}
              onChange={(e) => setOrigins(e.target.value)}
            />
            <p className="hint">一行一個。有填的話，只收從這些網站送來的表單；留白就不限制。</p>
          </div>
          <div className="sm:col-span-2">
            <label className="label" htmlFor="site-redirect">
              送出後轉去的網址
            </label>
            <input
              id="site-redirect"
              className="input"
              type="url"
              placeholder="https://www.example.com/thanks"
              value={redirectUrl}
              onChange={(e) => setRedirectUrl(e.target.value)}
            />
            <p className="hint">HTML 表單送出之後帶訪客去的頁面；留白就顯示內建的感謝頁。</p>
          </div>
          <label className="flex cursor-pointer items-start gap-3 text-sm sm:col-span-2">
            <input type="checkbox" className="mt-1 size-4 accent-brand-600" checked={active} onChange={(e) => setActive(e.target.checked)} />
            <span>
              收新的訊息
              <span className="block text-xs text-zinc-500 dark:text-zinc-400">關掉之後表單會被退回，已經收到的訊息還在。</span>
            </span>
          </label>
        </div>
      </Section>

      <Section title="負責人" hint="負責人看得到這個網站的訊息；管理員每個網站都看得到。">
        <div className="p-4 sm:p-5">
          <div className="max-h-56 overflow-y-auto rounded-lg border border-zinc-200 dark:border-zinc-800">
            {accounts
              .filter((a) => !a.disabled || members.has(a.id))
              .map((a) => (
                <label
                  key={a.id}
                  className="flex cursor-pointer items-center gap-3 border-b border-zinc-100 px-3 py-2 text-sm last:border-b-0 hover:bg-zinc-50 dark:border-zinc-800 dark:hover:bg-zinc-800/50"
                >
                  <input
                    type="checkbox"
                    className="size-4 accent-brand-600"
                    checked={members.has(a.id)}
                    onChange={() => setMembers((s) => toggle(s, a.id))}
                  />
                  {a.display_name}
                  <span className="font-mono text-xs text-zinc-500 dark:text-zinc-400">{a.username}</span>
                  {!a.email && <Badge tone="amber">沒填信箱</Badge>}
                  {a.disabled && <Badge tone="red">已停用</Badge>}
                </label>
              ))}
          </div>
        </div>
      </Section>

      <Section
        title="通知"
        hint="有新訊息的時候通知誰。"
        actions={
          <button type="button" className="btn btn-secondary" disabled={testing} onClick={() => void sendTest()}>
            {testing ? <Spinner /> : <BellRing className="size-4" />} 送測試通知
          </button>
        }
      >
        <div className="flex flex-col gap-4 p-4 sm:p-5">
          <label className="flex cursor-pointer items-start gap-3 text-sm">
            <input
              type="checkbox"
              className="mt-1 size-4 accent-brand-600"
              checked={notifyEmail}
              onChange={(e) => setNotifyEmail(e.target.checked)}
            />
            <span>
              寄信給負責人
              {meta && !meta.smtp_configured && (
                <span className="block text-xs text-amber-700 dark:text-amber-400">
                  伺服器還沒設定寄信（FI_SMTP_HOST），現在開了也寄不出去。
                </span>
              )}
              {notifyEmail && selectedWithoutEmail.length > 0 && (
                <span className="block text-xs text-amber-700 dark:text-amber-400">
                  {selectedWithoutEmail.map((a) => a.display_name).join("、")} 還沒填信箱，收不到信。
                </span>
              )}
            </span>
          </label>
          <div>
            <label className="label" htmlFor="site-webhook">
              Webhook 網址
            </label>
            <input
              id="site-webhook"
              className="input font-mono text-xs"
              type="url"
              autoComplete="off"
              placeholder="https://hooks.slack.com/services/…"
              value={webhookUrl}
              onChange={(e) => setWebhookUrl(e.target.value)}
            />
            <p className="hint">Slack 和 Discord 的 webhook 會自動用它們的格式；其他網址會收到完整的 JSON。</p>
          </div>
          <p className="text-xs text-zinc-500 dark:text-zinc-400">測試通知用的是已經存好的設定：改了記得先儲存。</p>
          {test && <TestOutcome result={test} />}
        </div>
      </Section>

      {error && <ErrorBanner message={error} />}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <button className="btn btn-primary" disabled={busy || !name.trim()}>
          {busy && <Spinner />} 儲存設定
        </button>
        <button type="button" className="btn btn-ghost text-rose-600 dark:text-rose-400" onClick={() => void remove()}>
          <Trash2 className="size-4" /> 刪除網站
        </button>
      </div>
    </form>
  );
}

/** 不是管理員的人看到的：只看不能改。 */
function Summary({ site }: { site: Site }) {
  return (
    <Section title="設定" hint="只有管理員可以改。">
      <dl className="grid gap-x-6 gap-y-3 p-4 text-sm sm:grid-cols-2 sm:p-5">
        <div>
          <dt className="text-xs text-zinc-500 dark:text-zinc-400">負責人</dt>
          <dd>{site.members.map((m) => m.display_name).join("、") || "—"}</dd>
        </div>
        <div>
          <dt className="text-xs text-zinc-500 dark:text-zinc-400">必填欄位</dt>
          <dd>{site.required_fields.map((f) => FIELD_LABEL[f]).join("、") || "沒有"}</dd>
        </div>
        <div>
          <dt className="text-xs text-zinc-500 dark:text-zinc-400">通知</dt>
          <dd>{[site.notify_email && "寄信給負責人", site.has_webhook && "Webhook"].filter(Boolean).join("、") || "沒有開"}</dd>
        </div>
        <div>
          <dt className="text-xs text-zinc-500 dark:text-zinc-400">允許的來源</dt>
          <dd className="break-all">{site.allowed_origins.join("、") || "不限制"}</dd>
        </div>
      </dl>
    </Section>
  );
}

export function SiteDetailPage() {
  const id = Number(useParams().id);
  const admin = useUser().role === "admin";
  const { data: site, setData, error } = useAsync(() => api.site(id), [id]);
  // 只有管理員拿得到帳號清單
  const accounts = useAsync(() => (admin ? api.users() : Promise.resolve(undefined)), [admin]);

  return (
    <PageBody>
      <Link to="/sites" className="mb-4 inline-flex items-center gap-1 text-sm text-zinc-500 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-white">
        <ArrowLeft className="size-4" /> 網站
      </Link>
      {(error || accounts.error) && <ErrorBanner message={error ?? accounts.error ?? ""} />}
      {!site && !error && <Loading />}
      {site && (
        <>
          <PageHeader
            title={
              <span className="flex flex-wrap items-center gap-2">
                {site.name} {site.active ? <Badge tone="green">收件中</Badge> : <Badge>已停收</Badge>}
              </span>
            }
            subtitle={`共 ${site.total.toLocaleString()} 則訊息，${site.unhandled.toLocaleString()} 則待處理`}
            actions={
              <Link className="btn btn-secondary" to={`/inbox?site_id=${site.id}`}>
                <Inbox className="size-4" /> 看訊息
              </Link>
            }
          />
          <div className="flex flex-col gap-4">
            <Embed site={site} onRotated={setData} />
            {admin ? (
              accounts.data && <SettingsForm key={site.id} site={site} accounts={accounts.data} onSaved={setData} />
            ) : (
              <Summary site={site} />
            )}
          </div>
        </>
      )}
    </PageBody>
  );
}
