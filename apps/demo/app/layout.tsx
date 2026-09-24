import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Perpetua · Streaming Payroll",
  description: "Reference dashboard for continuous payment streaming on Soroban.",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}