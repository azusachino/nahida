import type { AgentTool } from "@earendil-works/pi-agent-core";
import { Type } from "@earendil-works/pi-ai";
import type { Pack } from "../../pack";
import prompt from "./prompt.md" with { type: "text" };

const parameters = Type.Object({ text: Type.String() });

export const echoTool: AgentTool<typeof parameters> = {
  name: "echo",
  label: "Echo",
  description: "Returns the given text unchanged.",
  parameters,
  execute: async (_toolCallId, params) => ({
    content: [{ type: "text", text: params.text }],
    details: undefined,
  }),
};

/** Harmless pack that exercises the loader. The real packs come in later slices. */
export const echoPack: Pack = {
  id: "echo",
  tools: [echoTool],
  prompt: { file: "prompt.md", text: prompt },
};
