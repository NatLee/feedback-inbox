import { Navigate, Route, Routes } from "react-router";
import { Layout } from "./components/Layout";
import { ErrorBanner, Loading } from "./components/ui";
import { useApp } from "./context";
import { Dashboard } from "./pages/Dashboard";
import { FeedbackDetailPage } from "./pages/FeedbackDetail";
import { InboxPage } from "./pages/Inbox";
import { Login } from "./pages/Login";
import { Settings } from "./pages/Settings";
import { SiteDetailPage } from "./pages/SiteDetail";
import { Sites } from "./pages/Sites";

export function App() {
  const { auth, authError } = useApp();
  if (!auth) {
    return (
      <div className="mx-auto max-w-md px-4 py-10">
        {authError ? <ErrorBanner message={`連不上伺服器：${authError}`} /> : <Loading />}
      </div>
    );
  }
  // 還沒登入（或還沒有任何帳號）：只有登入頁
  if (!auth.user) return <Login setupRequired={auth.setup_required} />;
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route index element={<Dashboard />} />
        <Route path="inbox" element={<InboxPage />} />
        <Route path="inbox/:id" element={<FeedbackDetailPage />} />
        <Route path="sites" element={<Sites />} />
        <Route path="sites/:id" element={<SiteDetailPage />} />
        <Route path="settings" element={<Settings />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}
