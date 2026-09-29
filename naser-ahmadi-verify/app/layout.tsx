import type { Metadata, Viewport } from "next";
import "./ds/tokens.css";
import "./ds/components.css";
import "./ds/datepicker.css";
import "./app.css";

export const metadata: Metadata = {
  title: "Patient Verification",
  description: "Verify your driver's license and insurance before your appointment.",
  robots: { index: false, follow: false },
};

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  themeColor: "#FFFFFF",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <head>
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link rel="preconnect" href="https://fonts.gstatic.com" crossOrigin="" />
        {/* eslint-disable-next-line @next/next/no-page-custom-font */}
        <link
          rel="stylesheet"
          href="https://fonts.googleapis.com/css2?family=Inter+Tight:wght@400;500;600;700;800&family=Source+Serif+4:opsz,wght@8..60,400;8..60,500;8..60,600&family=JetBrains+Mono:wght@400;500&display=swap"
        />
      </head>
      <body className="ds-root">{children}</body>
    </html>
  );
}
