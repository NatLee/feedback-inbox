import type {
  Account,
  AuthState,
  FeedbackDetail,
  FeedbackFilter,
  FeedbackPage,
  Meta,
  Role,
  Site,
  SiteInput,
  Stats,
  Status,
  TestResult,
  User,
} from "./types";

/** session 過期或沒登入會收到 401；App 聽到這個事件就回到登入頁。 */
export const UNAUTHORIZED_EVENT = "fi:unauthorized";

/** 後端回 `{"error": "訊息"}` 時丟出這個，訊息直接給使用者看。 */
export class ApiError extends Error {
  readonly status: number;
  /** 完整的回應內容 */
  readonly data: unknown;
  constructor(status: number, message: string, data: unknown) {
    super(message);
    this.status = status;
    this.data = data;
  }
}

export async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const init: RequestInit = { method, headers: {} };
  if (body !== undefined) {
    init.body = JSON.stringify(body);
    (init.headers as Record<string, string>)["content-type"] = "application/json";
  }
  const res = await fetch(`/api${path}`, init);
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  let data: unknown = undefined;
  try {
    data = text ? JSON.parse(text) : undefined;
  } catch {
    data = text;
  }
  if (res.status === 401 && !path.startsWith("/auth/")) {
    window.dispatchEvent(new Event(UNAUTHORIZED_EVENT));
  }
  if (!res.ok) {
    const message =
      typeof data === "object" && data !== null && "error" in data
        ? String((data as { error: unknown }).error)
        : `伺服器回應 ${res.status}`;
    throw new ApiError(res.status, message, data);
  }
  return data as T;
}

const get = <T>(path: string) => request<T>("GET", path);
const post = <T>(path: string, body?: unknown) => request<T>("POST", path, body ?? {});
const patch = <T>(path: string, body: unknown) => request<T>("PATCH", path, body);
const del = <T = void>(path: string) => request<T>("DELETE", path);

function query(params: object): string {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== "") q.set(k, String(v));
  }
  const s = q.toString();
  return s ? `?${s}` : "";
}

/** 瀏覽器的時區比 UTC 快幾分鐘（台北是 480）；統計和匯出照這個切日。 */
const tzOffset = () => -new Date().getTimezoneOffset();

export const api = {
  meta: () => get<Meta>("/meta"),

  authState: () => get<AuthState>("/auth/state"),
  // 沒填的欄位就不送，後端會用帳號當名字
  setup: (input: { username: string; display_name: string; email: string; password: string }) =>
    post<User>("/auth/setup", {
      ...input,
      display_name: input.display_name || undefined,
      email: input.email || undefined,
    }),
  login: (username: string, password: string) => post<User>("/auth/login", { username, password }),
  logout: () => post<void>("/auth/logout"),
  updateMe: (input: { display_name?: string; email?: string }) => patch<User>("/auth/me", input),
  changePassword: (currentPassword: string, newPassword: string) =>
    post<void>("/auth/password", { current_password: currentPassword, new_password: newPassword }),

  users: () => get<Account[]>("/users"),
  createUser: (input: { username: string; display_name: string; email: string; password: string; role: Role }) =>
    post<Account>("/users", {
      ...input,
      display_name: input.display_name || undefined,
      email: input.email || undefined,
    }),
  updateUser: (
    id: number,
    input: { display_name?: string; email?: string; role?: Role; disabled?: boolean; password?: string },
  ) => patch<Account>(`/users/${id}`, input),

  sites: () => get<Site[]>("/sites"),
  site: (id: number) => get<Site>(`/sites/${id}`),
  createSite: (input: SiteInput) => post<Site>("/sites", input),
  updateSite: (id: number, input: SiteInput) => patch<Site>(`/sites/${id}`, input),
  deleteSite: (id: number) => del(`/sites/${id}`),
  rotateKey: (id: number) => post<Site>(`/sites/${id}/rotate-key`),
  testNotification: (id: number) => post<TestResult>(`/sites/${id}/test`),

  feedback: (filter: FeedbackFilter = {}) => get<FeedbackPage>(`/feedback${query(filter)}`),
  feedbackDetail: (id: number) => get<FeedbackDetail>(`/feedback/${id}`),
  setStatus: (id: number, status: Status) => patch<FeedbackDetail>(`/feedback/${id}`, { status }),
  bulkStatus: (ids: number[], status: Status) => post<{ updated: number }>("/feedback/bulk", { ids, status }),
  deleteFeedback: (id: number) => del(`/feedback/${id}`),
  addNote: (id: number, body: string) => post<FeedbackDetail>(`/feedback/${id}/notes`, { body }),
  deleteNote: (id: number) => del(`/notes/${id}`),
  exportUrl: (filter: FeedbackFilter) =>
    `/api/feedback/export${query({ ...filter, page: undefined, per_page: undefined, tz_offset: tzOffset() })}`,

  stats: () => get<Stats>(`/stats${query({ tz_offset: tzOffset() })}`),
};
