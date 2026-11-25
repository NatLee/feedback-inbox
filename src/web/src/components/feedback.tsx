import { MessageSquare } from "lucide-react";
import { Link } from "react-router";
import { STATUS_LABEL, STATUS_TONE, timeAgo } from "../lib/format";
import type { FeedbackItem, Status } from "../types";
import { Badge } from "./ui";

export function StatusBadge({ status }: { status: Status }) {
  return <Badge tone={STATUS_TONE[status]}>{STATUS_LABEL[status]}</Badge>;
}

/** 誰寫的：有名字用名字，沒有就用信箱。 */
export function senderName(item: { name: string; email: string }): string {
  return item.name || item.email || "（沒留名字）";
}

/** 一則訊息的摘要列（首頁「最新訊息」用）。 */
export function FeedbackRow({ item }: { item: FeedbackItem }) {
  return (
    <Link
      to={`/inbox/${item.id}`}
      className="flex items-start gap-3 px-4 py-3 transition-colors hover:bg-zinc-50 sm:px-5 dark:hover:bg-zinc-800/50"
    >
      <span className="mt-0.5 grid size-8 shrink-0 place-items-center rounded-full bg-brand-100 text-xs font-semibold text-brand-700 dark:bg-brand-950 dark:text-brand-300">
        {Array.from(senderName(item))[0]}
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex flex-wrap items-center gap-x-2 gap-y-1">
          <span className={`text-sm ${item.status === "new" ? "font-semibold" : "font-medium"}`}>{senderName(item)}</span>
          {item.company && <span className="text-xs text-zinc-500 dark:text-zinc-400">{item.company}</span>}
          <StatusBadge status={item.status} />
        </span>
        <span className="mt-0.5 block truncate text-sm text-zinc-600 dark:text-zinc-400">{item.preview}</span>
        <span className="mt-1 flex items-center gap-2 text-xs text-zinc-500 dark:text-zinc-400">
          {item.site_name}・{timeAgo(item.created_at)}
          {item.note_count > 0 && (
            <span className="inline-flex items-center gap-0.5" title={`${item.note_count} 則備註`}>
              <MessageSquare className="size-3" /> {item.note_count}
            </span>
          )}
        </span>
      </span>
    </Link>
  );
}
