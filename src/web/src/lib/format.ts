import type { BadgeTone } from "../components/ui";
import type { Field, Role, Status } from "../types";

export function dateTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString("zh-TW", { dateStyle: "medium", timeStyle: "short", hour12: false });
}

/** 「3 天前」這種寫法；超過一個月就給日期。 */
export function timeAgo(iso: string, now = Date.now()): string {
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return iso;
  const minutes = Math.floor((now - t) / 60_000);
  if (minutes < 1) return "剛剛";
  if (minutes < 60) return `${minutes} 分鐘前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小時前`;
  const days = Math.floor(hours / 24);
  if (days < 31) return `${days} 天前`;
  return new Date(iso).toLocaleDateString("zh-TW", { dateStyle: "medium" });
}

export const ROLE_LABEL: Record<Role, string> = { admin: "管理員", member: "成員" };

export const STATUSES: Status[] = ["new", "in_progress", "done", "spam"];

export const STATUS_LABEL: Record<Status, string> = {
  new: "未處理",
  in_progress: "處理中",
  done: "已完成",
  spam: "垃圾訊息",
};

export const STATUS_TONE: Record<Status, BadgeTone> = {
  new: "amber",
  in_progress: "sky",
  done: "green",
  spam: "neutral",
};

export const FIELDS: Field[] = ["name", "email", "phone", "company", "message"];

export const FIELD_LABEL: Record<Field, string> = {
  name: "姓名",
  email: "電子信箱",
  phone: "電話",
  company: "公司",
  message: "訊息",
};

/** 把一行一個（或用逗號隔開）的輸入拆成清單。 */
export function parseLines(input: string): string[] {
  const items: string[] = [];
  for (const raw of input.split(/[\n,，]+/)) {
    const item = raw.trim();
    if (item && !items.includes(item)) items.push(item);
  }
  return items;
}
