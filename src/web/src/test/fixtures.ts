import type { Account, FeedbackDetail, FeedbackItem, Meta, Site, Stats, User } from "../types";

export const meta: Meta = { version: "1.0.0", smtp_configured: true, public_url: "https://inbox.example.com" };

export const admin: User = { id: 1, username: "nat", display_name: "Nat", email: "nat@example.com", role: "admin" };
export const member: User = { id: 2, username: "amy", display_name: "Amy", email: "", role: "member" };

export const accounts: Account[] = [
  { ...admin, disabled: false, created_at: "2026-01-01T00:00:00Z", site_count: 1 },
  { ...member, disabled: false, created_at: "2026-01-02T00:00:00Z", site_count: 0 },
];

export const site: Site = {
  id: 7,
  name: "官方網站",
  url: "https://www.example.com",
  key: "abcd1234abcd1234",
  origin_mode: "exact",
  embed_origins: [],
  allowed_origins: ["https://www.example.com"],
  required_fields: ["name", "message"],
  redirect_url: "",
  webhook_url: "",
  has_webhook: false,
  notify_email: true,
  active: true,
  created_at: "2026-01-01T00:00:00Z",
  total: 12,
  unhandled: 3,
  members: [{ id: 1, username: "nat", display_name: "Nat", has_email: true }],
};

export function item(id: number, overrides: Partial<FeedbackItem> = {}): FeedbackItem {
  return {
    id,
    site_id: site.id,
    site_name: site.name,
    name: `訪客 ${id}`,
    email: `visitor${id}@example.com`,
    company: "",
    preview: `第 ${id} 則訊息`,
    status: "new",
    created_at: "2026-03-01T08:00:00Z",
    note_count: 0,
    ...overrides,
  };
}

export const detail: FeedbackDetail = {
  id: 5,
  site_id: site.id,
  site_name: site.name,
  name: "王小明",
  email: "ming@example.com",
  phone: "0900-000-000",
  company: "範例公司",
  message: "想了解報價。\n謝謝！",
  extra: [{ name: "budget", value: "10 萬" }],
  status: "new",
  ip: "203.0.113.9",
  user_agent: "Mozilla/5.0",
  page_url: "https://www.example.com/contact",
  created_at: "2026-03-01T08:00:00Z",
  updated_at: "2026-03-01T08:00:00Z",
  notes: [],
  deliveries: [{ channel: "webhook", target: "hooks.example.com", ok: false, detail: "HTTP 500", created_at: "2026-03-01T08:00:01Z" }],
};

export const emptyStats: Stats = {
  open_age: { fresh: 0, waiting: 0, overdue: 0 },
  by_status: { new: 0, in_progress: 0, done: 0, spam: 0 },
  last_7_days: 0,
  previous_7_days: 0,
  daily: [],
  sites: [],
  recent: [],
};
