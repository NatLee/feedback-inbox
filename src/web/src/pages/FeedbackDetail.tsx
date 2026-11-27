import {FeedbackWorkflow} from "../ai/FeedbackWorkflow";
import {
  ArrowLeft,
  Building2,
  CheckCircle2,
  ExternalLink,
  Mail,
  Phone,
  Send,
  Trash2,
  XCircle,
} from "lucide-react";
import { useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api } from "../api";
import { StatusBadge, senderName } from "../components/feedback";
import { PageBody } from "../components/Layout";
import { ErrorBanner, Loading, Spinner, useToast } from "../components/ui";
import { useUser } from "../context";
import { STATUSES, STATUS_LABEL, dateTime, timeAgo } from "../lib/format";
import { errorText, useAsync } from "../lib/hooks";
import type { Delivery, FeedbackDetail, Note, Status } from "../types";

const CHANNEL_LABEL: Record<Delivery["channel"], string> = {
  email: "通知信",
  webhook: "Webhook",
};

function Contact({ detail }: { detail: FeedbackDetail }) {
  const rows = [
    detail.email && {
      icon: Mail,
      text: detail.email,
      href: `mailto:${detail.email}`,
    },
    detail.phone && {
      icon: Phone,
      text: detail.phone,
      href: `tel:${detail.phone.replace(/[^\d+]/g, "")}`,
    },
    detail.company && { icon: Building2, text: detail.company },
  ].filter((r) => !!r);
  if (rows.length === 0) return null;
  return (
    <ul className="mt-3 flex flex-wrap gap-x-5 gap-y-2 text-sm">
      {rows.map(({ icon: Icon, text, href }) => (
        <li
          key={text}
          className="flex items-center gap-2 text-zinc-600 dark:text-zinc-300"
        >
          <Icon className="size-4 text-zinc-400" />
          {href ? (
            <a
              className="hover:text-brand-700 hover:underline dark:hover:text-brand-300"
              href={href}
            >
              {text}
            </a>
          ) : (
            text
          )}
        </li>
      ))}
    </ul>
  );
}

function Notes({
  detail,
  onChange,
}: {
  detail: FeedbackDetail;
  onChange: (next?: FeedbackDetail) => void;
}) {
  const me = useUser();
  const toast = useToast();
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);

  const add = async () => {
    setBusy(true);
    try {
      onChange(await api.addNote(detail.id, body.trim()));
      setBody("");
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };

  const remove = async (note: Note) => {
    if (!window.confirm("刪除這則備註？")) return;
    try {
      await api.deleteNote(note.id);
      onChange();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };

  return (
    <section className="handling-notes card" aria-label="處理歷史">
      <h2 className="border-b border-zinc-100 px-4 py-3 text-sm font-semibold sm:px-5 dark:border-zinc-800">
        <span className="section-index">04 / 處理歷史</span>處理紀錄
      </h2>
      <ol className="flex flex-col gap-4 px-4 py-4 sm:px-5">
        <li className="flex gap-3 text-sm text-zinc-500 dark:text-zinc-400">
          <span className="mt-2 size-2 shrink-0 rounded-full bg-zinc-300 dark:bg-zinc-600" />
          <span>
            從「{detail.site_name}」送進來・{dateTime(detail.created_at)}
          </span>
        </li>
        {detail.notes.map((note) =>
          note.kind === "status" ? (
            <li
              key={note.id}
              className="flex gap-3 text-sm text-zinc-500 dark:text-zinc-400"
            >
              <span className="mt-2 size-2 shrink-0 rounded-full bg-brand-400" />
              <span>
                <span className="font-semibold text-zinc-700 dark:text-zinc-200">
                  {note.author}
                </span>{" "}
                改成「
                {STATUS_LABEL[note.body as Status] ?? note.body}」・
                {timeAgo(note.created_at)}
              </span>
            </li>
          ) : (
            <li key={note.id} className="flex gap-3">
              <span className="grid size-7 shrink-0 place-items-center rounded-full bg-brand-100 text-xs font-semibold text-brand-700 dark:bg-brand-950 dark:text-brand-300">
                {Array.from(note.author)[0]}
              </span>
              <div className="min-w-0 flex-1 rounded-lg bg-zinc-50 px-3 py-2 dark:bg-zinc-800/60">
                <div className="flex items-center justify-between gap-2 text-xs text-zinc-500 dark:text-zinc-400">
                  <span>
                    <span className="font-semibold text-zinc-700 dark:text-zinc-200">
                      {note.author}
                    </span>
                    ・
                    <span title={dateTime(note.created_at)}>
                      {timeAgo(note.created_at)}
                    </span>
                  </span>
                  {(note.user_id === me.id || me.role === "admin") && (
                    <button
                      className="btn btn-ghost -mr-2 px-1 py-1"
                      title="刪除備註"
                      onClick={() => void remove(note)}
                    >
                      <Trash2 className="size-4" />
                    </button>
                  )}
                </div>
                <p className="prose-message mt-1">{note.body}</p>
              </div>
            </li>
          ),
        )}
      </ol>
      <form
        className="border-t border-zinc-100 p-4 sm:px-5 dark:border-zinc-800"
        onSubmit={(e) => {
          e.preventDefault();
          void add();
        }}
      >
        <label className="sr-only" htmlFor="note-body">
          新增備註
        </label>
        <textarea
          id="note-body"
          className="input min-h-20"
          maxLength={2000}
          placeholder="寫給同事看的備註：聯絡過了嗎、對方怎麼說…（訪客看不到）"
          value={body}
          onChange={(e) => setBody(e.target.value)}
        />
        <div className="mt-2 flex justify-end">
          <button className="btn btn-primary" disabled={busy || !body.trim()}>
            {busy ? <Spinner /> : <Send className="size-4" />} 新增備註
          </button>
        </div>
      </form>
    </section>
  );
}

function Side({ detail }: { detail: FeedbackDetail }) {
  return (
    <>
      <section className="card p-4 text-sm">
        <h2 className="mb-3 text-sm font-semibold">來源</h2>
        <dl className="flex flex-col gap-3">
          <div>
            <dt className="text-xs text-zinc-500 dark:text-zinc-400">網站</dt>
            <dd>
              <Link
                className="text-brand-700 hover:underline dark:text-brand-300"
                to={`/sites/${detail.site_id}`}
              >
                {detail.site_name}
              </Link>
            </dd>
          </div>
          <div>
            <dt className="text-xs text-zinc-500 dark:text-zinc-400">時間</dt>
            <dd>{dateTime(detail.created_at)}</dd>
          </div>
          {detail.page_url && (
            <div>
              <dt className="text-xs text-zinc-500 dark:text-zinc-400">
                從哪一頁送出
              </dt>
              <dd className="break-all">
                {/^https?:\/\//.test(detail.page_url) ? (
                  <a
                    className="inline-flex items-start gap-1 text-brand-700 hover:underline dark:text-brand-300"
                    href={detail.page_url}
                    target="_blank"
                    rel="noreferrer noopener"
                  >
                    {detail.page_url}{" "}
                    <ExternalLink className="mt-1 size-4 shrink-0" />
                  </a>
                ) : (
                  detail.page_url
                )}
              </dd>
            </div>
          )}
          {detail.ip && (
            <div>
              <dt className="text-xs text-zinc-500 dark:text-zinc-400">IP</dt>
              <dd className="font-mono text-xs">{detail.ip}</dd>
            </div>
          )}
          {detail.user_agent && (
            <div>
              <dt className="text-xs text-zinc-500 dark:text-zinc-400">
                瀏覽器
              </dt>
              <dd className="text-xs break-words text-zinc-600 dark:text-zinc-400">
                {detail.user_agent}
              </dd>
            </div>
          )}
        </dl>
      </section>

      <section className="card p-4 text-sm">
        <h2 className="mb-3 text-sm font-semibold">通知</h2>
        {detail.deliveries.length === 0 ? (
          <p className="text-xs text-zinc-500 dark:text-zinc-400">
            這則訊息沒有送出通知。
          </p>
        ) : (
          <ul className="flex flex-col gap-3">
            {detail.deliveries.map((d, i) => (
              <li key={i} className="flex items-start gap-2">
                {d.ok ? (
                  <CheckCircle2 className="mt-1 size-4 shrink-0 text-emerald-600 dark:text-emerald-400" />
                ) : (
                  <XCircle className="mt-1 size-4 shrink-0 text-rose-600 dark:text-rose-400" />
                )}
                <span className="min-w-0">
                  <span className="block">
                    {CHANNEL_LABEL[d.channel] ?? d.channel}
                    <span className="ml-2 text-xs text-zinc-500 dark:text-zinc-400">
                      {d.ok ? "已送出" : "失敗"}
                    </span>
                  </span>
                  <span className="block text-xs break-all text-zinc-500 dark:text-zinc-400">
                    {d.target}
                  </span>
                  {!d.ok && d.detail && (
                    <span className="block text-xs break-words text-rose-600 dark:text-rose-400">
                      {d.detail}
                    </span>
                  )}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </>
  );
}

export function FeedbackDetailPage() {
  const id = Number(useParams().id);
  const me = useUser();
  const toast = useToast();
  const navigate = useNavigate();
  const { data, setData, error, reload } = useAsync(
    () => api.feedbackDetail(id),
    [id],
  );
  const [busy, setBusy] = useState(false);

  const setStatus = async (status: Status) => {
    setBusy(true);
    try {
      setData(await api.setStatus(id, status));
      toast(`改成「${STATUS_LABEL[status]}」`);
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!window.confirm("刪除這則訊息？備註和通知紀錄也會一起刪掉，不能復原。"))
      return;
    try {
      await api.deleteFeedback(id);
      toast("訊息刪除了");
      navigate("/inbox");
    } catch (e) {
      toast(errorText(e), "error");
    }
  };

  return (
    <PageBody>
      <Link
        to="/inbox"
        className="mb-4 inline-flex items-center gap-1 text-sm text-zinc-500 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-white"
      >
        <ArrowLeft className="size-4" /> 收件匣
      </Link>
      {error && <ErrorBanner message={error} />}
      {!data && !error && <Loading />}
      {data && (
        <div className="letter-workspace grid gap-4 lg:grid-cols-[1fr_18rem]">
          <div className="flex min-w-0 flex-col gap-4">
            <section className="message-paper card p-4 sm:p-5" key={data.id} aria-label="來信閱讀">
              <div className="letter-letterhead">
                <span>
                  03 / 來信閱讀
                  <small>來信原文 / #{String(data.id).padStart(5, "0")}</small>
                </span>
                <span className="received-stamp">
                  RECEIVED
                  <small>
                    {new Date(data.created_at).toLocaleDateString("zh-TW")}
                  </small>
                </span>
              </div>
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div className="min-w-0">
                  <h1 className="flex flex-wrap items-center gap-2 text-xl font-semibold tracking-tight">
                    {senderName(data)} <StatusBadge status={data.status} />
                  </h1>
                  <Contact detail={data} />
                  <p className="hint mt-3">{data.identity?.email_verified?'會員 Email 已由網站後端驗證':'聯絡資料由訪客自行填寫，尚未驗證'}{data.identity?.subject&&` · 會員 ${data.identity.subject}`}</p>
                  {data.is_test&&<span className="ai-stamp mt-2">安裝測試訊息</span>}
                </div>
                <div className="flex items-center gap-2">
                  {busy && <Spinner />}
                  <select
                    className="input w-auto"
                    aria-label="狀態"
                    value={data.status}
                    disabled={busy}
                    onChange={(e) => void setStatus(e.target.value as Status)}
                  >
                    {STATUSES.map((s) => (
                      <option key={s} value={s}>
                        {STATUS_LABEL[s]}
                      </option>
                    ))}
                  </select>
                  {me.role === "admin" && (
                    <button
                      className="btn btn-ghost px-2 text-rose-600 dark:text-rose-400"
                      title="刪除訊息"
                      onClick={() => void remove()}
                    >
                      <Trash2 className="size-4" />
                    </button>
                  )}
                </div>
              </div>
              <p className="letter-state" key={data.status}>
                <span className="letter-state-mark" aria-hidden="true" />
                目前狀態 · {STATUS_LABEL[data.status]}
              </p>
              <p className="prose-message mt-5 border-t border-zinc-100 pt-4 dark:border-zinc-800">
                {data.message || (
                  <span className="text-zinc-500 dark:text-zinc-400">
                    （沒有寫訊息）
                  </span>
                )}
              </p>
              {data.form && <dl className="mt-4 grid gap-x-4 gap-y-2 rounded-lg bg-zinc-50 p-3 text-sm sm:grid-cols-[auto_1fr] dark:bg-zinc-800/60">{data.form.schema.fields.filter(f=>!['name','email','phone','company','message'].includes(f.key)).map(f=><div className="contents" key={f.id}><dt className="text-zinc-500">{f.label}</dt><dd className="break-words whitespace-pre-wrap">{typeof data.answers?.[f.id]==='boolean'?(data.answers[f.id]?'是':'否'):(f.type==='select'?f.options.find(o=>o.value===data.answers?.[f.id])?.label:undefined)??String(data.answers?.[f.id]??'—')}</dd></div>)}<dt className="hint">來源表單版本</dt><dd className="hint">v{data.form.version}</dd></dl>}
              {!data.form && data.extra.length > 0 && (
                <dl className="mt-4 grid gap-x-4 gap-y-2 rounded-lg bg-zinc-50 p-3 text-sm sm:grid-cols-[auto_1fr] dark:bg-zinc-800/60">
                  {data.extra.map((field) => (
                    <div key={field.name} className="contents">
                      <dt className="font-mono text-xs leading-6 text-zinc-500 dark:text-zinc-400">
                        {field.name}
                      </dt>
                      <dd className="break-words whitespace-pre-wrap">
                        {field.value}
                      </dd>
                    </div>
                  ))}
                </dl>
              )}
            </section>
            <FeedbackWorkflow detail={data} admin={me.role==="admin"} onClassified={reload}/>
            <Notes
              detail={data}
              onChange={(next) => (next ? setData(next) : void reload())}
            />
          </div>
          <aside className="flex flex-col gap-4">
            <Side detail={data} />
          </aside>
        </div>
      )}
    </PageBody>
  );
}
