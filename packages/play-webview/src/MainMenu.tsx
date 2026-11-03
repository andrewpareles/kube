import { useState } from "react";
import type { GraphicsQuality } from "./hostBridge";

const GRAPHICS_QUALITY_OPTIONS: { graphicsQuality: GraphicsQuality; label: string }[] = [
  { graphicsQuality: "low", label: "Normal" },
  { graphicsQuality: "high", label: "Ultra" },
];

// Asked before leaving, in place of the menu. Clicking outside it goes back to the menu.
const LeaveConfirmation = ({ onLeave, onCancel }: { onLeave: () => void; onCancel: () => void }) => (
  <div
    data-ui-only=""
    onClick={(event) => {
      event.stopPropagation();
      onCancel();
    }}
    className="absolute inset-0 flex items-center justify-center"
  >
    <div onClick={(event) => event.stopPropagation()} className="flex w-64 flex-col gap-6 rounded-md bg-neutral-900/90 p-10 text-white shadow-lg">
      <p className="font-semibold">Leave this game?</p>
      <div className="flex gap-2">
        <button type="button" onClick={onCancel} className="flex-1 rounded-md bg-white/10 py-2 hover:bg-white/15">
          Cancel
        </button>
        <button type="button" onClick={onLeave} className="flex-1 rounded-md bg-white py-2 font-medium text-neutral-900 hover:bg-neutral-200">
          Leave
        </button>
      </div>
    </div>
  </div>
);

// TODO: AI-PICKED-VALUE: A 384-point panel with 40-point padding in the middle of the screen over a 50% black backdrop. It fades in and out over 150 ms. Clicking the backdrop closes it.
export const MAIN_MENU_FADE_MS = 150;

export const MainMenu = ({
  isOpen,
  volume,
  onVolumeChange,
  graphicsQuality,
  onGraphicsQualityChange,
  onLeave,
  onClose,
}: {
  isOpen: boolean;
  volume: number;
  onVolumeChange: (volume: number) => void;
  graphicsQuality: GraphicsQuality;
  onGraphicsQualityChange: (graphicsQuality: GraphicsQuality) => void;
  onLeave: () => void;
  onClose: () => void;
}) => {
  const [isConfirmingLeave, setIsConfirmingLeave] = useState(false);
  const [sliderVolume, setSliderVolume] = useState(volume);
  return (
    <div data-ui-only="" onClick={onClose} className={`absolute inset-0 z-[1000] flex items-center justify-center bg-black/50 transition-opacity duration-[${MAIN_MENU_FADE_MS}ms] ease-out starting:opacity-0 ${isOpen ? "" : "pointer-events-none opacity-0"}`}>
      <div onClick={(event) => event.stopPropagation()} className={`${isConfirmingLeave ? "hidden" : "flex"} w-96 flex-col gap-6 rounded-md bg-neutral-900/90 p-10 text-white shadow-lg`}>
        <p className="text-center text-lg font-semibold">Settings</p>
        <label className="flex items-center justify-between gap-4 text-sm">
          <span>Volume</span>
          <span className="flex items-center gap-3">
            <input
              type="range"
              min={0}
              max={1}
              step="any"
              value={sliderVolume}
              onChange={(event) => {
                setSliderVolume(Number(event.currentTarget.value));
                onVolumeChange(Math.round(Number(event.currentTarget.value) * 100) / 100);
              }}
              style={{ background: `linear-gradient(to right, #737373 ${sliderVolume * 100}%, #404040 ${sliderVolume * 100}%)` }}
              className="h-1.5 w-36 appearance-none rounded-full outline-none [&::-webkit-slider-thumb]:size-4 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-neutral-400"
            />
            <span className="w-10 text-right text-neutral-400 tabular-nums">{Math.round(volume * 100)}%</span>
          </span>
        </label>
        <div className="flex items-center justify-between gap-4 text-sm">
          <span>Graphics</span>
          <div className="flex w-49 gap-1 rounded-md bg-white/10 p-1">
            {GRAPHICS_QUALITY_OPTIONS.map((option) => (
              <button
                key={option.graphicsQuality}
                type="button"
                aria-pressed={option.graphicsQuality === graphicsQuality}
                onClick={() => onGraphicsQualityChange(option.graphicsQuality)}
                className={`flex-1 rounded-md py-1 ${option.graphicsQuality === graphicsQuality ? "bg-white/20" : "hover:bg-white/10"}`}
              >
                {option.label}
              </button>
            ))}
          </div>
        </div>
        <div className="mt-4 flex flex-col gap-3">
          <button type="button" onClick={onClose} className="rounded-md bg-white/10 py-2 hover:bg-white/15">
            Back to Game
          </button>
          <button type="button" onClick={() => setIsConfirmingLeave(true)} className="rounded-md bg-white/10 py-2 hover:bg-white/15">
            Leave Game
          </button>
        </div>
      </div>
      {isConfirmingLeave && <LeaveConfirmation onLeave={onLeave} onCancel={() => setIsConfirmingLeave(false)} />}
    </div>
  );
};
