import { ProviderButtons } from "../access/ProviderButtons";
import { LogIn, UserPlus } from "lucide-react";
import { useRef, useState } from "react";
import { ErrorBanner, Spinner } from "../components/ui";
import { useApp } from "../context";
import { errorText } from "../lib/hooks";

/** Shared themed account surface; lifecycle is owned by the route pages. */
export function AuthScreen({ setupRequired }: { setupRequired: boolean }) {
  const { login, setup, meta } = useApp();
  const [username, setUsername] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const submitting = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    if (submitting.current) return;
    setError(null);
    if (setupRequired) {
      if (!/^[a-zA-Z0-9._-]{3,32}$/.test(username.trim())) { setError("帳號要 3 到 32 個字元，只能用英數字和 _ . -"); return; }
      if ([...password].length < 8 || [...password].length > 200) { setError("密碼需為 8 到 200 個字元"); return; }
      if ([...displayName.trim()].length > 40) { setError("顯示名稱最多 40 個字元"); return; }
      if (password !== confirmation) { setError("兩次輸入的密碼不一致"); return; }
    }
    submitting.current = true;
    setBusy(true);
    try {
      if (setupRequired) {
        await setup({
          username: username.trim(),
          display_name: displayName.trim(),
          email: email.trim(),
          password,
        });
      } else {
        await login(username.trim(), password);
      }
      setPassword("");
      setConfirmation("");
    } catch (e) {
      setError(errorText(e));
      if (!setupRequired) setPassword("");
    } finally {
      submitting.current = false;
      setBusy(false);
    }
  };

  return (
    <div className="desk-login">
      <section className="login-story">
        <span className="eyebrow">FEEDBACK INBOX / EST. FOR YOUR TEAM</span>
        <div className="login-envelope" aria-hidden="true">
          <span>Dear team,</span>
          <i>
            每個聲音，
            <br />
            都有回音。
          </i>
          <small>YOURS, YOUR CUSTOMERS</small>
        </div>
        <h2>
          讓每一封來信，
          <br />
          都找到回應。
        </h2>
        <p>
          網站表單的訊息、團隊的備註、每一步處理進度。
          <br />
          一張工作桌，把關係照顧好。
        </p>
      </section>
      <div className="login-form-wrap">
        <form
          className="card w-full max-w-sm p-6"
          aria-busy={busy}
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <div className="mb-6 flex items-center gap-3">
            <img src="/favicon.svg" alt="" className="size-10" />
            <div>
              <h1 className="font-semibold tracking-tight">{setupRequired ? "建立第一位管理員" : "Feedback Inbox"}</h1>
              <p className="text-xs text-zinc-500 dark:text-zinc-400">
                {setupRequired
                  ? "首次啟用 / FIRST LOGIN"
                  : "網站表單的收件匣"}
                {meta ? `・v${meta.version}` : ""}
              </p>
            </div>
          </div>
          {setupRequired && <div className="first-login-note">
            <p className="eyebrow">01 建立帳號 / 02 邀請團隊</p>
            <p>由你保管工作空間的第一把鑰匙。建立後即可登入，再由管理員新增其他成員。</p>
          </div>}
          <label className="label" htmlFor="login-username">
            帳號
          </label>
          <input
            id="login-username"
            className="input"
            autoComplete="username"
            autoFocus
            value={username}
            onChange={(e) => setUsername(e.target.value)}
          />
          {setupRequired && (
            <>
              <p className="hint">3 到 32 個字元：英文字母、數字、. _ -</p>
              <label className="label mt-3" htmlFor="login-display-name">
                顯示名稱
              </label>
              <input
                id="login-display-name"
                className="input"
                placeholder="其他人看到的名字，可以不填"
                value={displayName}
                onChange={(e) => setDisplayName(e.target.value)}
              />
              <label className="label mt-3" htmlFor="login-email">
                電子信箱
              </label>
              <input
                id="login-email"
                className="input"
                type="email"
                autoComplete="email"
                placeholder="收新訊息通知用，可以不填"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
              />
            </>
          )}
          <label className="label mt-3" htmlFor="login-password">
            密碼
          </label>
          <input
            id="login-password"
            className="input"
            type="password"
            autoComplete={setupRequired ? "new-password" : "current-password"}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
          {setupRequired && <>
            <p className="hint">8 到 200 個字元。</p>
            <label className="label mt-3" htmlFor="login-confirmation">確認密碼</label>
            <input id="login-confirmation" className="input" type="password" autoComplete="new-password" value={confirmation} onChange={e=>setConfirmation(e.target.value)} />
          </>}
          {error && (
            <div className="mt-4">
              <ErrorBanner message={error} />
            </div>
          )}
          <button
            className="btn btn-primary mt-5 w-full py-2"
            type="submit"
            disabled={busy || !username.trim() || !password || (setupRequired && !confirmation)}
          >
            {busy ? (
              <Spinner />
            ) : setupRequired ? (
              <UserPlus className="size-4" />
            ) : (
              <LogIn className="size-4" />
            )}
            {setupRequired ? "建立管理員" : "登入"}
          </button>
          <p className="mt-4 text-xs text-zinc-500 dark:text-zinc-400">
            {setupRequired
              ? "這個帳號可以新增網站和其他成員。之後這個畫面就不會再出現。"
              : "沒有帳號或忘記密碼，請找管理員。"}
          </p>
          {!setupRequired && <ProviderButtons/>}
        </form>
        <p className="login-footer">
          A LITTLE MORE PERSONAL. A LOT MORE ORGANIZED.
        </p>
      </div>
    </div>
  );
}
