"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import { LogOut, Menu, Settings } from "lucide-react";
import posthog from "posthog-js";
import { FaDiscord } from "react-icons/fa";
import { usePlayerLook, usePlayerName, usePlayerUserId, useUser } from "@/components/context/useUser";
import { clearDesktopAuthSessions } from "@/app/server-actions/auth";
import { DISCORD_URL, isResourceNavigationItemActive, siteHeaderSignedInResourceNavigationItems, siteHeaderTopNavigationItems, siteResourceNavigationItems } from "@/lib/siteNavigation";
// import { CREDIT_ICON, CREDIT_ICON_CLASS_NAME, dailyFreeReward } from "@/lib/pricing";
import { HoverTooltip } from "@/components/site/HoverTooltip";
import { supabaseOnClient } from "@/supabase/supabaseOnClient";
import { SiteBrandLink } from "@/components/site/SiteBrandLink";
import { PlayerAvatar } from "@/components/site/PlayerAvatar";
import { SiteSidebar } from "@/app/SiteSidebar";
import { openLoginModal } from "@/app/login/LoginModal";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shadcn/components/ui/dropdown-menu";
import { Sheet, SheetContent, SheetTitle, SheetTrigger } from "@/shadcn/components/ui/sheet";

// Every item in the header row is a tab: it runs the full height of the header, so the whole
// column above and below the words can be clicked, and they sit edge to edge so there is no dead
// strip between them. The blue bar underneath is the only mark of which one you are on, and it
// is the deeper of the logo's two blues. It is drawn as its own bar rather than as a border,
// which can only ever be a square-ended line. Hovering darkens the words and nothing else, so a
// tab you are merely passing over never looks like the tab you are on.
//
// The bar is always there and always blue. Moving between tabs grows one up out of the bottom
// edge while the other sinks back into it: it grows by its height, with its bottom edge pinned
// and only its top corners rounded, so it reads as rising out of the edge of the header rather
// than as something sitting on top of it. It runs the whole width of the tab, which is what makes
// the tabs read as a row of them rather than as words that happen to be underlined.
//
// Whether a tab shows at all is left to each one, because a tab carrying both `inline-flex` and
// `hidden` has two display utilities fighting and the loser is decided by the order Tailwind
// happens to write them out, not by the order they are listed in.
// The prototype replaces the tab bars with compact links and a blue current section.
const HEADER_TAB_CLASS_NAME = "site-nav-link";

const getHeaderTabStateClassName = (isCurrentPage: boolean) => isCurrentPage
  ? "text-kube-blue"
  : "text-foreground";

const LoginButton = () => (
  <button
    type="button"
    onClick={() => openLoginModal({ next: `${window.location.pathname}${window.location.search}` })}
    className="site-login"
  >
    Log in
  </button>
);

// Where the sidebar goes on a phone. It carries the same links in the same order, top group then
// bottom, so the menu and the rail are the one list read two ways. The placeholders are left out:
// they are already unpressable in the rail, and a menu row that does nothing is worse.
// Both navigation groups move into this menu together when the header no longer has room.
// The drawer is the sidebar itself, slid in from the left, so the two can never show different links. Following any
// link or button in it closes it. It covers the whole screen, header included.
const MobileNavigationMenu = () => {
  const [isOpen, setIsOpen] = useState(false);

  return (
    <Sheet open={isOpen} onOpenChange={setIsOpen}>
      <SheetTrigger asChild>
        <button
          type="button"
          aria-label="Open menu"
          className="site-menu-trigger size-10 shrink-0 items-center justify-center rounded-lg"
        >
          <Menu className="size-6" />
        </button>
      </SheetTrigger>
      <SheetContent
        side="left"
        aria-describedby={undefined}
        className="site-sidebar-drawer"
        onClick={(event) => {
          if (event.target instanceof Element && event.target.closest("a, button") !== null) setIsOpen(false);
        }}
      >
        <SheetTitle className="sr-only">Menu</SheetTitle>
        <SiteSidebar brand={<SiteBrandLink renderId="site-drawer" logoSidePx={48} />} />
      </SheetContent>
    </Sheet>
  );
};

// // Capitalised so JSX reads it as a component, the same way the nav icons are used below.
// const CreditIcon = CREDIT_ICON;
//
// // The whole balance is the link to the plans page, and it carries where you were so checkout can
// // send you back to it.
// export const AccountBalance = ({ credits, nextPath, onNavigate }: { credits: number, nextPath: string, onNavigate?: (event: { preventDefault: () => void }) => void }) => (
//   <HoverTooltip
//     side="bottom"
//     className=""
//     trigger={
//       <Link href={getPlansUrl({ next: nextPath })} onNavigate={onNavigate} className="site-balance" aria-label={`${credits.toLocaleString()} credits. See plans.`}>
//         <span className="site-balance-inner">
//           <CreditIcon className={`size-4 ${CREDIT_ICON_CLASS_NAME}`} aria-hidden="true" />
//           {credits.toLocaleString()}
//         </span>
//       </Link>
//     }
//   >
//     <span className="site-balance-daily">
//       <span>Come back daily for</span>
//       {/* The period rides inside the amount so the row's gap does not push it off. */}
//       <span className="site-balance-daily-amount">
//         <CreditIcon className={`site-balance-daily-icon ${CREDIT_ICON_CLASS_NAME}`} aria-hidden="true" />
//         {dailyFreeReward.credits}.
//       </span>
//     </span>
//   </HoverTooltip>
// );

const LogOutMenuItem = ({ className }: { className: string }) => {
  const router = useRouter();
  const [isLoggingOut, setIsLoggingOut] = useState(false);

  const handleLogout = async () => {
    setIsLoggingOut(true);
    try {
      try {
        await clearDesktopAuthSessions();
      } catch (error) {
        console.error("Failed to clear desktop auth sessions", error);
      }
      const supabase = supabaseOnClient();
      const { error } = await supabase.auth.signOut();
      if (error) {
        throw error;
      }
      posthog.reset();
      router.refresh();
    } catch (error) {
      console.error("Failed to log out", error);
    } finally {
      setIsLoggingOut(false);
    }
  };

  return (
    <DropdownMenuItem
      disabled={isLoggingOut}
      onSelect={(event) => {
        event.preventDefault();
        void handleLogout();
      }}
      className={className}
    >
      <LogOut className="size-4" />
      {isLoggingOut ? "Logging Out..." : "Log Out"}
    </DropdownMenuItem>
  );
};

const UserSettingsMenu = () => (
  <DropdownMenu modal={false}>
    <DropdownMenuTrigger asChild>
      <button
        type="button"
        aria-label="Open settings"
        className="site-settings-trigger inline-flex size-7 items-center justify-center transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-gray-950 focus-visible:ring-offset-2"
      >
        <Settings className="size-4" />
      </button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end" className="w-40">
      <LogOutMenuItem className="cursor-pointer" />
    </DropdownMenuContent>
  </DropdownMenu>
);

export const SiteHeader = () => {
  const user = useUser();
  const pathname = usePathname();
  const headerRef = useRef<HTMLElement>(null);
  const surfaceRef = useRef<HTMLDivElement>(null);
  // The header's white surface turns on once the page scrolls, and on the homepage only
  // once the header reaches the headline. It is set on the element rather than in state, so
  // scrolling never re-renders the header.
  useEffect(() => {
    const updateIsScrolled = () => {
      const headline = document.querySelector(".create-hero .create-headline");
      const headerHeight = headerRef.current?.offsetHeight ?? 0;
      const scrollYWhereSurfaceTurnsOn = headline === null ? 0 : Math.max(0, headline.getBoundingClientRect().top + window.scrollY - headerHeight);
      if (surfaceRef.current !== null) surfaceRef.current.dataset.scrolled = String(window.scrollY > scrollYWhereSurfaceTurnsOn);
    };
    updateIsScrolled();
    window.addEventListener("scroll", updateIsScrolled, { passive: true });
    window.addEventListener("resize", updateIsScrolled);
    return () => {
      window.removeEventListener("scroll", updateIsScrolled);
      window.removeEventListener("resize", updateIsScrolled);
    };
  }, [pathname]);
  const playerName = usePlayerName();
  const playerUserId = usePlayerUserId();
  // const credits = useCredits();
  const look = usePlayerLook();
  // Only ever the username, never the email address behind it: an email is not a name, and it is
  // not the account holder's to have shown on a screen they might not be alone at.
  const playerProfilePath = playerUserId === null ? null : `/profile/${playerUserId}`;

  return (
    <header
      ref={headerRef}
      className="site-header"
    >
      {/* Each page gets its own white surface, so it switches with the page instead of fading. */}
      <div key={pathname} ref={surfaceRef} aria-hidden={true} className="site-header-surface" />
      {/* The mark sits alone, centred on the 80px sidebar rail so it keeps lining up with the
          sidebar icons below. The row of header items then begins exactly where that rail ends,
          which is also where every page's own content begins. */}
      {/* On a phone every item along the row carries its own px-4, and so does the row, so each
          one is 32px from whatever is beside it and the two on the ends are 32px from the edge of
          the screen. Spacing lives on the items rather than in gaps between them, which is why
          there is nothing here to keep in step with them. */}
      <div className="site-header-inner">
        {/* The rail is what centres the mark, and the link is only the mark itself, so the whole
            of what can be clicked is also the whole of what answers to the pointer.

            The 80px of it only exists to line the mark up with the sidebar, and on a phone there
            is no sidebar to line it up with, so it drops to hugging the mark. */}
        <span className="site-header-brand shrink-0">
          <MobileNavigationMenu />
          <SiteBrandLink renderId="site-header" logoSidePx={48} />
        </span>

        {/* The name is dropped on a phone, where there is only room for the mark, one link and the
            log in button. The mark already leads home, so the link is the one that does not. */}
        <nav className="site-primary-nav" aria-label="Main navigation">
          {/* The name stays black wherever you are, because dimming it to the same grey as a
               section would read as the site losing its own name. The bar still marks it. */}
          {siteHeaderTopNavigationItems.map((navigationItem) => (
            <Link
              key={navigationItem.label}
              href={navigationItem.href}
              aria-current={isResourceNavigationItemActive(pathname, navigationItem) ? "page" : undefined}
              className={`${HEADER_TAB_CLASS_NAME} ${getHeaderTabStateClassName(isResourceNavigationItemActive(pathname, navigationItem))}`}
            >
              {navigationItem.label === "Catalog" ? "Hats" : navigationItem.label}
            </Link>
          ))}
          {user && siteHeaderSignedInResourceNavigationItems.map((navigationItem) => (
            <Link key={navigationItem.label} href={navigationItem.href} aria-current={isResourceNavigationItemActive(pathname, navigationItem) ? "page" : undefined} className={`${HEADER_TAB_CLASS_NAME} ${getHeaderTabStateClassName(isResourceNavigationItemActive(pathname, navigationItem))}`}>
              {navigationItem.label}
            </Link>
          ))}
        </nav>

        <HoverTooltip
          side="bottom"
          className=""
          trigger={
            <a href={DISCORD_URL} target="_blank" rel="noreferrer noopener nofollow" aria-label="Discord" className="site-discord-link">
              <span className="site-discord-inner">
                <FaDiscord className="size-5" />
              </span>
            </a>
          }
        >
          Join the Discord community.
        </HoverTooltip>

        {/* Signed out there is no sidebar, so the resources it would hold sit on the right of the header. */}
        {!user && (
          <nav className="site-resource-nav" aria-label="Resources">
            {siteResourceNavigationItems.map((navigationItem) => (
              <Link key={navigationItem.label} href={navigationItem.href} aria-current={isResourceNavigationItemActive(pathname, navigationItem) ? "page" : undefined} className={`${HEADER_TAB_CLASS_NAME} ${getHeaderTabStateClassName(isResourceNavigationItemActive(pathname, navigationItem))}`}>
                {navigationItem.label}
              </Link>
            ))}
          </nav>
        )}

        {/* Wider apart on a phone, so the account controls and the menu keep the same 16px that
            the mark and each tab do. From md up there is room for them to sit closer again. */}
        <div className="site-account-nav">
          {user ? (
            <>
              {/* {credits !== null && (
                <AccountBalance credits={credits} nextPath={pathname} />
              )} */}
              {playerName !== null && playerProfilePath !== null && (
                <Link
                  href={playerProfilePath}
                  className="site-header-name hidden max-w-40 items-center gap-2 text-[14px] text-foreground lg:flex"
                >
                  {look !== null && <PlayerAvatar look={look} isProfileAvatar={false} className="size-7" />}
                  <span className="truncate">{playerName}</span>
                </Link>
              )}
              <UserSettingsMenu />
            </>
          ) : (
            <>
              <LoginButton />
            </>
          )}
        </div>
      </div>
    </header>
  );
};
