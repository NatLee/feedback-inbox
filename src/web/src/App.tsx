import { Register } from "./pages/Register";
import { Connections } from "./access/Connections";
import { Authorize } from "./access/Authorize";
import { ContinueAuthorization } from "./access/ContinueAuthorization";
import { safeAuthorizeNext, providerErrorMessage } from "./access/navigation";
import { Navigate, Route, Routes, useLocation } from "react-router";
import { Layout, PageBody } from "./components/Layout";
import { ErrorBanner, Loading } from "./components/ui";
import { useApp } from "./context";
import { Dashboard } from "./pages/Dashboard";
import { FeedbackDetailPage } from "./pages/FeedbackDetail";
import { InboxPage } from "./pages/Inbox";
import { Login } from "./pages/Login";
import { FirstLogin } from "./pages/FirstLogin";
import { Settings } from "./pages/Settings";
import { SiteDetailPage } from "./pages/SiteDetail";
import { Sites } from "./pages/Sites";

export function App() {
  const { auth, authError } = useApp();
  const location=useLocation();
  const next=safeAuthorizeNext(location.search);
  if (!auth) {
    return (
      <div className="mx-auto max-w-md px-4 py-10">
        {authError ? <ErrorBanner message={`連不上伺服器：${authError}`} /> : <Loading />}
      </div>
    );
  }
  if (!auth.user && !auth.setup_required && location.pathname === "/register") return <Register />;
  if (!auth.user) {
    const path = auth.setup_required ? "/first-login" : "/login";
    const params = new URLSearchParams();
    if (next) params.set("next", next);
    if (!auth.setup_required && providerErrorMessage(location.search)) {
      params.set("access_error", new URLSearchParams(location.search).get("access_error")!);
    }
    if (!auth.setup_required && new URLSearchParams(location.search).get("registration") === "pending") params.set("registration", "pending");
    const search = params.size ? `?${params}` : "";
    // Canonicalize entry and drop untrusted continuation parameters.
    if (location.pathname !== path || location.search !== search) return <Navigate to={path + search} replace />;
    return auth.setup_required ? <FirstLogin /> : <Login />;
  }
  if(next)return <ContinueAuthorization to={next}/>;
  if (["/login", "/first-login", "/register"].includes(location.pathname)) return <Navigate to="/" replace />;
  return (
    <Routes>
      <Route path="access/authorize" element={<Authorize/>}/>
      <Route element={<Layout />}>
        <Route path="connections" element={<PageBody><Connections isAdmin={auth.user.role === "admin"}/></PageBody>}/>
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
