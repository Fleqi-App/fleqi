import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { createPreviewAdapter } from "./adapters/host/preview";
import { HostProvider } from "./store/host";
import { ModelsPage } from "./pages/settings/ModelsPage";

async function setup() {
  const adapter = createPreviewAdapter({ delayMs: 0 });
  await adapter.providerSave({ id: "saved", displayName: "测试连接", baseUrl: "https://example.com/v1", models: ["custom-model"], defaultGenerationModel: "custom-model", summaryModel: null, timeoutMs: 5000, apiKey: "fixture-key" });
  const user = userEvent.setup();
  render(<HostProvider adapter={adapter}><ModelsPage /></HostProvider>);
  await user.click(await screen.findByTestId("provider-edit-saved"));
  return { adapter, user, dialog: screen.getByRole("dialog", { name: "模型连接" }) };
}

describe("模型连接管理", () => {
  it("编辑时使用已保存凭据和配置超时，获取模型保留手动模型及用途", async () => {
    const { adapter, user, dialog } = await setup();
    const probe = vi.spyOn(adapter, "providerProbe").mockResolvedValue({ ok: true, models: ["remote-model"], error: null });
    await user.click(within(dialog).getByRole("button", { name: /获取模型|连接检查/ }));
    await waitFor(() => expect(probe).toHaveBeenCalledWith("https://example.com/v1", null, 5000, { providerId: "saved" }));
    expect((within(dialog).getByLabelText("默认生成模型") as HTMLSelectElement).value).toBe("custom-model");
    await user.click(within(dialog).getByRole("button", { name: "保存端点" }));
    await waitFor(async () => expect((await adapter.providerList())[0]!.record.models).toEqual(["custom-model", "remote-model"]));
  });

  it("无需模型列表接口也可手动添加、搜索并选择摘要模型", async () => {
    const { adapter, user, dialog } = await setup();
    await user.type(within(dialog).getByLabelText("手动模型 ID"), "manual-model");
    await user.click(within(dialog).getByRole("button", { name: "添加模型" }));
    await user.type(within(dialog).getByLabelText("搜索模型"), "manual");
    expect(within(dialog).getByTestId("model-row-manual-model")).toBeTruthy();
    expect(within(dialog).queryByTestId("model-row-custom-model")).toBeNull();
    await user.selectOptions(within(dialog).getByLabelText("摘要模型（可选）"), "manual-model");
    await user.click(within(dialog).getByRole("button", { name: "保存端点" }));
    await waitFor(async () => expect((await adapter.providerList())[0]!.record.summaryModel).toBe("manual-model"));
  });

  it("修改地址后忽略旧探测回执，保存失败在编辑框内显示并保留草稿", async () => {
    const { adapter, user, dialog } = await setup();
    let resolve!: (result: { ok: boolean; models: string[]; error: string | null }) => void;
    vi.spyOn(adapter, "providerProbe").mockImplementation(() => new Promise((done) => { resolve = done; }));
    await user.click(within(dialog).getByRole("button", { name: /获取模型|连接检查/ }));
    const url = within(dialog).getByLabelText("端点地址（HTTPS 或本机回环）");
    await user.clear(url);
    await user.type(url, "https://other.example/v1");
    resolve({ ok: true, models: ["stale-model"], error: null });
    await waitFor(() => expect(within(dialog).queryByText(/获取成功/)).toBeNull());
    await user.click(within(dialog).getByLabelText("保存时清除已有密钥"));
    vi.spyOn(adapter, "providerSave").mockRejectedValue({ code: "storage", message: "写入失败", retryable: false });
    await user.click(within(dialog).getByRole("button", { name: "保存端点" }));
    expect(await within(dialog).findByText(/写入失败/)).toBeTruthy();
    expect((url as HTMLInputElement).value).toBe("https://other.example/v1");
    expect(within(dialog).queryByTestId("model-row-stale-model")).toBeNull();
  });

  it("模型调用与获取列表分开，取消后的失败回执不写回界面", async () => {
    const { adapter, user, dialog } = await setup();
    let resolve!: (result: { ok: boolean; models: string[]; error: string | null }) => void;
    const probe = vi.spyOn(adapter, "providerProbe").mockImplementation(() => new Promise((done) => { resolve = done; }));
    await user.click(within(dialog).getByRole("button", { name: "测试模型" }));
    expect(probe).toHaveBeenCalledWith("https://example.com/v1", null, 5000, { providerId: "saved", model: "custom-model" });
    await user.click(within(dialog).getByRole("button", { name: "取消检查" }));
    resolve({ ok: false, models: [], error: "过期错误" });
    await waitFor(() => expect(within(dialog).queryByText("过期错误")).toBeNull());
    expect((within(dialog).getByLabelText("默认生成模型") as HTMLSelectElement).value).toBe("custom-model");
  });

  it("地址变化不能隐式复用旧凭据，删除选定模型会同步清理用途", async () => {
    const { adapter, user, dialog } = await setup();
    const probe = vi.spyOn(adapter, "providerProbe");
    await user.clear(within(dialog).getByLabelText("端点地址（HTTPS 或本机回环）"));
    await user.type(within(dialog).getByLabelText("端点地址（HTTPS 或本机回环）"), "https://other.example/v1");
    await user.click(within(dialog).getByRole("button", { name: "获取模型" }));
    expect(probe).not.toHaveBeenCalled();
    expect(within(dialog).getByText(/地址已修改，请重新输入密钥/)).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "移除模型 custom-model" }));
    expect((within(dialog).getByLabelText("默认生成模型") as HTMLSelectElement).value).toBe("");
    await user.click(within(dialog).getByRole("button", { name: "取消编辑" }));
    await user.click(screen.getByRole("button", { name: "继续编辑" }));
    expect(screen.getByRole("dialog", { name: "模型连接" })).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "取消编辑" }));
    await user.click(screen.getByRole("button", { name: "放弃修改" }));
    expect(screen.queryByRole("dialog", { name: "模型连接" })).toBeNull();
    expect((await adapter.providerList())[0]!.record.defaultGenerationModel).toBe("custom-model");
  });
});
