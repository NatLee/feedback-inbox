import { ProviderButtons } from "../access/ProviderButtons";
import { LogIn, UserPlus } from "lucide-react";
import { useState } from "react";
import { ErrorBanner, Spinner } from "../components/ui";
import { useApp } from "../context";
import { errorText } from "../lib/hooks";

/**
 * 還沒登入時整個畫面就是這一頁。資料庫裡還沒有任何帳號時（`setupRequired`），
 * 改成請第一個進來的人建立管理員：沒有預設帳號，也沒有預設密碼。
 */
export function Login({ setupRequired }: { setupRequired: boolean }) {
  const { login, setup, meta } = useApp();
  const [username, setUsername] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    setBusy(true);
    setError(null);
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
    } catch (e) {
      setError(errorText(e));
      if (!setupRequired) setPassword("");
    } finally {
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
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <div className="mb-6 flex items-center gap-3">
            <img src="/favicon.svg" alt="" className="size-10" />
            <div>
              <h1 className="font-semibold tracking-tight">Feedback Inbox</h1>
              <p className="text-xs text-zinc-500 dark:text-zinc-400">
                {setupRequired
                  ? "第一次使用：建立管理員帳號"
                  : "網站表單的收件匣"}
                {meta ? `・v${meta.version}` : ""}
              </p>
            </div>
          </div>
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
          {setupRequired && <p className="hint">至少 8 個字元。</p>}
          {error && (
            <div className="mt-4">
              <ErrorBanner message={error} />
            </div>
          )}
          <button
            className="btn btn-primary mt-5 w-full py-2"
            type="submit"
            disabled={busy || !username.trim() || !password}
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
