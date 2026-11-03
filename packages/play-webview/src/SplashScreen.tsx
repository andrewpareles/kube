import { useState } from "react";
import type { SplashScreenContent } from "./hostBridge";
import { LoadingKubeMark, RestingKubeMark } from "./LoadingKubeMark";

// The splash screen shows while a game is loading or connecting, and when something went wrong.

// TODO: AI-PICKED-VALUE: This sets the displayed splash image container size so logos stay readable on laptop and desktop windows.
const SPLASH_IMAGE_SIDE_PX = 220;
// TODO: AI-PICKED-VALUE: This keeps the title text large enough to read without overpowering the logo card.
const SPLASH_TITLE_TEXT_SIZE_PX = 40;
// TODO: AI-PICKED-VALUE: This keeps the subtitle visibly secondary to the title while still readable from normal viewing distance.
const SPLASH_SUB_TEXT_SIZE_PX = 20;
// TODO: AI-PICKED-VALUE: Long game names and error details wrap only past 560px, about the width of a short sentence at the title size.
const SPLASH_TEXT_MAX_WIDTH_PX = 560;
// TODO: AI-PICKED-VALUE: Room around the mark and words inside the panel behind them.
const SPLASH_PANEL_SIDE_PADDING_PX = 64;
const SPLASH_PANEL_TOP_AND_BOTTOM_PADDING_PX = 40;
// TODO: AI-PICKED-VALUE: Most game names fit inside this width, so most games get the same size panel and only long names widen it.
const SPLASH_PANEL_MIN_WIDTH_PX = 560;
// TODO: AI-PICKED-VALUE: This spacing separates the image card from text so the heading block reads as a separate section.
const SPLASH_TEXT_TOP_SPACING_PX = 18;
// TODO: AI-PICKED-VALUE: This creates a small gap between title and subtitle while keeping them visually grouped.
const SPLASH_SUB_TEXT_TOP_SPACING_PX = 8;
// TODO: AI-PICKED-VALUE: How far a background thumbnail is washed towards white, which is far enough
// that the mark and the words on top of it stay easy to read.
const SPLASH_THUMBNAIL_WASH_PERCENT = 82;
// TODO: AI-PICKED-VALUE: The screen is a very faint wash rather than flat white, running at the same
// angle as the gradient along the mark's own edges so the two read as one piece of artwork. The tint
// is only a few steps off white, which is enough to stop a large empty screen looking like a blank.
// CSS measures gradient angles from straight up rather than from the right, so the mark's 42 degrees is 132 here.
const SPLASH_BACKGROUND_GRADIENT_ANGLE_DEGREES = 132;
const SPLASH_BACKGROUND_GRADIENT_START_COLOR = "#ffffff";
const SPLASH_BACKGROUND_GRADIENT_END_COLOR = "#e9ebfa";
// TODO: AI-PICKED-VALUE: A thumbnail arrives whenever the download finishes, so it is faded up over
// this long to keep it from snapping onto the screen part-way through the wait. It covers the whole
// screen, and a change that large wants longer than a small one to stop reading as a jump.
const SPLASH_BACKGROUND_FADE_IN_MS = 800;

// The thumbnail fills the screen, cropped to keep its own shape. The wash sits on it and the two fade
// in together as one layer; faded separately, the screen would dip darker part-way through.
const SplashThumbnail = ({ thumbnailUrl }: { thumbnailUrl: string }) => {
  const [isLoaded, setIsLoaded] = useState(false);
  return (
    <div
      className={`absolute inset-0 transition-opacity duration-[${SPLASH_BACKGROUND_FADE_IN_MS}ms] ease-in-out ${isLoaded ? "opacity-100" : "opacity-0"}`}
    >
      <img src={thumbnailUrl} onLoad={() => setIsLoaded(true)} className="size-full object-cover" />
      <div className={`absolute inset-0 bg-white/${SPLASH_THUMBNAIL_WASH_PERCENT}`} />
    </div>
  );
};

export const SplashScreen = ({ splashScreenContent }: { splashScreenContent: SplashScreenContent }) => {
  const { titleText, subText, thumbnailUrl, isLoading } = splashScreenContent;
  return (
    <div
      className={`absolute inset-0 bg-[linear-gradient(${SPLASH_BACKGROUND_GRADIENT_ANGLE_DEGREES}deg,${SPLASH_BACKGROUND_GRADIENT_START_COLOR},${SPLASH_BACKGROUND_GRADIENT_END_COLOR})]`}
    >
      {thumbnailUrl !== null && <SplashThumbnail key={thumbnailUrl} thumbnailUrl={thumbnailUrl} />}
      <div className="absolute inset-0 flex items-center justify-center text-center antialiased">
        <div
          className={`flex min-w-[${SPLASH_PANEL_MIN_WIDTH_PX}px] flex-col items-center rounded-3xl bg-neutral-500/12 px-[${SPLASH_PANEL_SIDE_PADDING_PX}px] py-[${SPLASH_PANEL_TOP_AND_BOTTOM_PADDING_PX}px] backdrop-blur-md`}
        >
          {isLoading ? <LoadingKubeMark sidePx={SPLASH_IMAGE_SIDE_PX} renderId="splash" /> : <RestingKubeMark sidePx={SPLASH_IMAGE_SIDE_PX} renderId="splash" />}
          <p className={`mt-[${SPLASH_TEXT_TOP_SPACING_PX}px] max-w-[${SPLASH_TEXT_MAX_WIDTH_PX}px] text-[${SPLASH_TITLE_TEXT_SIZE_PX}px] leading-tight font-[620] tracking-[-0.045em] text-balance text-[#172554]`}>{titleText}</p>
          {subText !== null && (
            <p
              className={`mt-[${SPLASH_SUB_TEXT_TOP_SPACING_PX}px] max-w-[${SPLASH_TEXT_MAX_WIDTH_PX}px] text-[${SPLASH_SUB_TEXT_SIZE_PX}px] leading-snug text-balance text-neutral-500`}
            >
              {subText}
            </p>
          )}
        </div>
      </div>
    </div>
  );
};
