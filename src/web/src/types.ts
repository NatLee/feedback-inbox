export type Role = "admin" | "member";
export type Status = "new" | "in_progress" | "done" | "spam";

export interface Meta {
  version: string;
  /** 有沒有設定寄信（FI_SMTP_HOST）；沒有的話通知信寄不出去 */
  smtp_configured: boolean;
  /** 對外的網址，組表單網址用；沒設定是空字串 */
  public_url: string;
}

export interface User {
  id: number;
  username: string;
  display_name: string;
  /** 收通知信的信箱；沒填是空字串 */
  email: string;
  role: Role;
}

export interface AuthState {
  /** 還沒有任何帳號：第一個進來的人要先建立管理員 */
  setup_required: boolean;
  user: User | null;
}

export interface Account extends User {
  disabled: boolean;
  created_at: string;
  /** 負責幾個網站 */
  site_count: number;
}

export interface Member {
  id: number;
  username: string;
  display_name: string;
  /** 沒填信箱就收不到通知信 */
  has_email: boolean;
}

/** 表單裡認得的欄位 */
export type Field = "name" | "email" | "phone" | "company" | "message";

export interface Site {
  id: number;
  name: string;
  url: string;
  /** 表單網址是 `/f/<key>` */
  key: string;
  allowed_origins: string[];
  required_fields: Field[];
  redirect_url: string;
  /** 只有管理員拿得到；其他人是空字串 */
  webhook_url: string;
  has_webhook: boolean;
  notify_email: boolean;
  active: boolean;
  created_at: string;
  total: number;
  unhandled: number;
  members: Member[];
}

export interface SiteInput {
  name?: string;
  url?: string;
  allowed_origins?: string[];
  required_fields?: Field[];
  redirect_url?: string;
  webhook_url?: string;
  notify_email?: boolean;
  active?: boolean;
  member_ids?: number[];
}

export interface Outcome {
  channel: "email" | "webhook";
  target: string;
  ok: boolean;
  detail: string;
}

export interface TestResult {
  outcomes: Outcome[];
  smtp_configured: boolean;
  /** 開了通知信，但負責人都沒填信箱 */
  no_recipients: boolean;
}

export interface FeedbackItem {
  id: number;
  site_id: number;
  site_name: string;
  name: string;
  email: string;
  company: string;
  /** 訊息的開頭，壓成一行 */
  preview: string;
  status: Status;
  created_at: string;
  note_count: number;
}

export interface FeedbackPage {
  items: FeedbackItem[];
  total: number;
  page: number;
  per_page: number;
}

/** `open` 是還沒處理完的（未處理＋處理中）；不給的話是垃圾訊息以外的全部 */
export type StatusFilter = Status | "open" | "all";

export type FeedbackAge = "fresh" | "waiting" | "overdue";

export interface FeedbackFilter {
  age?: FeedbackAge;
  sort?: "newest" | "oldest";
  site_id?: number;
  status?: StatusFilter;
  q?: string;
  from?: string;
  to?: string;
  page?: number;
  per_page?: number;
}

export interface Note {
  id: number;
  user_id: number;
  author: string;
  /** `status` 是改狀態時自動留下的紀錄，body 是新的狀態 */
  kind: "note" | "status";
  body: string;
  created_at: string;
}

export interface Delivery extends Outcome {
  created_at: string;
}

export interface FeedbackDetail {
  id: number;
  site_id: number;
  site_name: string;
  name: string;
  email: string;
  phone: string;
  company: string;
  message: string;
  /** 表單裡其他的欄位，照送來的順序 */
  extra: { name: string; value: string }[];
  status: Status;
  ip: string;
  user_agent: string;
  page_url: string;
  created_at: string;
  updated_at: string;
  notes: Note[];
  deliveries: Delivery[];
}

export interface SiteSummary {
  id: number;
  name: string;
  active: boolean;
  total: number;
  unhandled: number;
  last_at: string | null;
}

export interface Stats {
  open_age: Record<FeedbackAge, number>;
  by_status: Record<Status, number>;
  last_7_days: number;
  previous_7_days: number;
  /** 最近 30 天每天幾則，舊的在前 */
  daily: { date: string; count: number }[];
  sites: SiteSummary[];
  recent: FeedbackItem[];
}
