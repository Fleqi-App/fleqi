import { config as base } from "./wdio.conf";
export const config: WebdriverIO.Config = {
  ...base,
  specs: ["./specs/windows-core.spec.ts"],
  mochaOpts: { ui: "bdd", timeout: 60_000 },
};
