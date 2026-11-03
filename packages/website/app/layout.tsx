import type { Metadata } from "next";
import { Suspense } from "react";
import localFont from "next/font/local";
import "./globals.css";
import { Toaster } from "@/shadcn/components/ui/sonner";
import { UserProvider } from "@/components/context/useUser";
import { getSiteAccount } from "@/lib/siteAccount";
import { getUser } from "@/lib/siteUser";
import { SiteLayoutShell } from "./SiteLayoutShell";
import { LoginModal } from "./login/LoginModal";
import { ReactQueryProvider } from "./ReactQueryProvider";
import { SiteFooter } from "./SiteFooter";
import { SITE_NAME, SITE_ORIGIN } from "@/lib/siteMetadata";

// Geist is drawn from the same geometric skeleton as the cube-K: flat terminals, even stroke,
// and a strong diagonal on letters like the K itself. It also pairs with the Geist Mono the
// site already loads, so headings, body and code all read as one family.
// The selected homepage uses the local Inter variable font throughout the site.
const inter = localFont({
  src: [
    { path: "../../../assets/fonts/InterVariable.ttf", style: "normal" },
    { path: "../../../assets/fonts/InterVariable-Italic.woff2", style: "italic" },
  ],
  variable: "--font-inter",
  weight: "100 900",
  display: "swap",
});

const geistMono = localFont({
  src: "../../../assets/fonts/GeistMono-Regular.ttf",
  variable: "--font-geist-mono",
  weight: "400",
  display: "swap",
});

export const metadata: Metadata = {
  metadataBase: new URL(SITE_ORIGIN),
  title: {
    default: SITE_NAME,
    template: `%s | ${SITE_NAME}`,
  },
  robots: {
    index: true,
    follow: true,
    googleBot: {
      index: true,
      follow: true,
      "max-image-preview": "large",
      "max-snippet": -1,
      "max-video-preview": -1,
    },
  },
};

export default async function Layout({ children }: Readonly<{ children: React.ReactNode; }>) {
  const user = await getUser()
  const account = user === null ? null : await getSiteAccount({ profileId: user.id })

  // The font variables have to sit on <html>, because Tailwind resolves --font-sans there. On
  // <body> they are declared too late for it to see them, and every page falls back to the
  // default system stack instead of the font we loaded.
  return (
    <html lang="en" className={`${inter.variable} ${geistMono.variable}`}>
      <body
        suppressHydrationWarning={true}
        className="antialiased"
      >
        <ReactQueryProvider>
          <UserProvider user={user} account={account}>
            {/* <LoginOptionsForm /> */}

            <SiteLayoutShell footer={<SiteFooter />}>
              {children}
            </SiteLayoutShell>
            {/* The modal reads the url, which static pages like /privacy only have once they reach the browser. */}
            <Suspense fallback={null}>
              <LoginModal />
            </Suspense>
            <Toaster />
          </UserProvider>
        </ReactQueryProvider>
      </body>
    </html>
  );
}
