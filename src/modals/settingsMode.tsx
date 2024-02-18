import { Signal, signal } from "@preact/signals";

import { invoke } from "@tauri-apps/api/tauri";

import {
  border,
  enchantedForestGradient,
  enchantedForestGradientActive,
  overlay,
} from "../ui/app.css";

import { Icon } from "../ui/icons";

import { Modes } from "./types";
import { Stack } from "../types";

interface Settings {
  shortcut: Record<string, boolean> | null;
  enterBehavior: "c" | "p";
}
const saved: Signal<Settings> = signal({
  shortcut: null,
  enterBehavior: "c",
});
(async () => {
  saved.value.shortcut = await invoke("spotlight_get_shortcut");
  saved.value.enterBehavior =
    (await invoke("spotlight_get_enter_behavior")) || "c";
})();

export default {
  name: (_: Stack) => "Settings",
  hotKeys: (_stack: Stack, modes: Modes) => [
    {
      name: "Back",
      keys: ["ESC"],
      onMouseDown: () => modes.deactivate(),
    },
  ],
  Modal: ({}: { stack: Stack; modes: Modes }) => {
    if (!saved.value) return;
    const options = [
      ["shift", "IconShiftKey"],
      ["ctrl", "IconCtrlKey"],
      ["alt", "IconAltKey"],
      ["command", "IconCommandKey"],
    ];

    return (
      <div
        className={overlay}
        style={{
          position: "absolute",
          overflow: "auto",
          width: "40ch",
          height: "auto",
          fontSize: "0.9rem",
          bottom: "0",
          right: "4ch",
          padding: "1ch 2ch 1ch 2ch",
          borderRadius: "0.5rem 0.5rem 0 0",
          display: "flex",
          flexDirection: "column",
          gap: "1ch",
          zIndex: 1000,
        }}
      >
        <div style={{ display: "flex", flexDirection: "column" }}>
          <p>Enter Behavior</p>
          <select
            value={saved.value.enterBehavior}
            onChange={(e) => {
              saved.value.enterBehavior = e.currentTarget
                .value as Settings["enterBehavior"];
              invoke("spotlight_update_enter_behavior", {
                behavior: e.currentTarget.value,
              });
            }}
          >
            <option value="c">Copy</option>
            <option value="p">Paste if possible, else copy</option>
          </select>
        </div>
        <div style={{ display: "flex", flexDirection: "column" }}>
          <p>Activation Shortcut</p>
          <div
            style={{
              display: "flex",
              gap: "1ch",
              alignItems: "center",
              textAlign: "right",
            }}
          >
            {options.map(([name, icon]) => (
              <div
                onMouseDown={() => {
                  let update = saved.peek()?.shortcut ?? {};
                  update[name] = !update[name];
                  console.log(`${name}: `, update);
                  invoke("spotlight_update_shortcut", { shortcut: update });
                  saved.value.shortcut = { ...update };
                }}
                className={
                  border +
                  " " +
                  (saved.value.shortcut && saved.value.shortcut[name]
                    ? enchantedForestGradientActive
                    : enchantedForestGradient)
                }
              >
                <span
                  style="
            display: inline-block;
            width: 1.5em;
            height: 1.5em;
            text-align: center;
            border-radius: 5px;
            "
                >
                  {<Icon name={icon} />}
                </span>
              </div>
            ))}
            + SPACE
          </div>
        </div>
      </div>
    );
  },
};
