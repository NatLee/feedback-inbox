import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, test } from "vitest";
import { fakeApi } from "../test/fakeApi";
import { accounts, admin, member, meta, site } from "../test/fixtures";
import { renderApp } from "../test/render";

const signedIn = (user = admin) => ({
  "GET /meta": () => meta,
  "GET /auth/state": () => ({ setup_required: false, user }),
  "GET /sites/7": () => site,
  "GET /sites/7/form": () => ({published:{id:1,site_id:7,version:1,schema:{schema_version:1,title:site.name,description:"",submit_label:"送出",success_message:"收到",fields:[]}},draft:{revision:1,schema:{schema_version:1,title:site.name,description:"",submit_label:"送出",success_message:"收到",fields:[]}}}),
  "GET /sites/7/integrations": () => ({version:1,code:"<form></form>"}),
});

test("表單網址用對外的網址組，管理員可以改設定", async () => {
  const calls = fakeApi({
    ...signedIn(),
    "GET /users": () => accounts,
    "PATCH /sites/7": (body) => ({ ...site, ...(body as object) }),
  });
  renderApp("/sites/7");

  await userEvent.click(await screen.findByRole("button",{name:"發布與嵌入"}));
  expect(await screen.findByText("https://inbox.example.com/f/abcd1234abcd1234")).toBeInTheDocument();

  await userEvent.click(screen.getByRole("button",{name:"通知與負責人"}));
  await userEvent.click(screen.getByLabelText(/Amy/));
  await userEvent.type(screen.getByLabelText("允許的來源"), "\nhttps://example.com");
  await userEvent.click(screen.getByRole("button", { name: "儲存設定" }));

  expect(await screen.findByRole("status")).toHaveTextContent("設定存好了");
  expect(calls.find((c) => c.method === "PATCH")?.body).toMatchObject({
    name: "官方網站",
    allowed_origins: ["https://www.example.com", "https://example.com"],
    member_ids: [1, 2],
    notify_email: true,
  });
  expect((calls.find((c) => c.method === "PATCH")?.body as Record<string,unknown>).required_fields).toBeUndefined();
  // Amy 沒填信箱，開著通知信要提醒
  expect(screen.getByText(/Amy 還沒填信箱/)).toBeInTheDocument();
});

test("測試通知會列出每個目標的結果", async () => {
  fakeApi({
    ...signedIn(),
    "GET /users": () => accounts,
    "POST /sites/7/test": () => ({
      outcomes: [
        { channel: "email", target: "nat@example.com", ok: true, detail: "" },
        { channel: "webhook", target: "hooks.example.com", ok: false, detail: "HTTP 404" },
      ],
      smtp_configured: true,
      no_recipients: false,
    }),
  });
  renderApp("/sites/7");

  await userEvent.click(await screen.findByRole("button",{name:"通知與負責人"}));
  await userEvent.click(await screen.findByRole("button", { name: "送測試通知" }));
  expect(await screen.findByText(/nat@example.com/)).toBeInTheDocument();
  expect(screen.getByText("HTTP 404")).toBeInTheDocument();
});

test("成員只能看設定，不能改", async () => {
  fakeApi(signedIn(member));
  renderApp("/sites/7");

  await userEvent.click(await screen.findByRole("button",{name:"通知與負責人"}));
  expect(await screen.findByText("只有管理員可以改。")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "儲存設定" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /換網址/ })).not.toBeInTheDocument();
});
