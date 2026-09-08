import * as React from "react";

export function X(props: React.SVGProps<SVGSVGElement>) {
  // Official X (Twitter) logo SVG, scaled to 24x24 viewBox for consistent sizing
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      width={24}
      height={24}
      aria-hidden="true"
      {...props}
    >
      <title>X</title>
      {/* The path is scaled down from the original 1200x1227 to fit 24x24 */}
      <path
        d="M14.283 10.16L22.018 0H20.1l-6.6 8.91L7.07 0H0l9.378 13.6L0 24h1.918l6.86-8.91L16.855 24H24l-9.71-13.84zm-2.418 4.01l-.77-1.09L2.88 1.56h3.27l6.04 8.6.77 1.09 7.09 10.01h-3.27l-6.09-8.6z"
        fill="currentColor"
      />
    </svg>
  );
}
