import type { Metadata } from "next";

import "./globals.css";
import { Providers } from "@ps/providers";

export const metadata: Metadata = {
  title: "Prism",
  description: "Engineering insights platform",
};

const RootLayout = ({ children }: { children: React.ReactNode }) => {
  return (
    <html lang="en">
      <body className="min-h-screen bg-background text-foreground antialiased">
        <Providers>{children}</Providers>
      </body>
    </html>
  );
};

export default RootLayout;
