import { AppShell } from "@/components/app-shell";

import "./globals.css";
import { cn } from "@/lib/utils";
import type { Metadata } from "next";
import { Geist } from "next/font/google";

import { Providers } from "@ps/providers";

const geist = Geist({ subsets: ["latin"], variable: "--font-sans" });

export const metadata: Metadata = {
  title: "Prism",
  description: "Engineering insights platform",
};

const RootLayout = ({ children }: { children: React.ReactNode }) => {
  return (
    <html lang="en" className={cn("font-sans", geist.variable)}>
      <body className="min-h-screen bg-background text-foreground antialiased">
        <Providers>
          <AppShell>{children}</AppShell>
        </Providers>
      </body>
    </html>
  );
};

export default RootLayout;
