import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/utils/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center whitespace-normal rounded-full text-[15px] font-semibold ring-offset-background transition-colors duration-150 ease-out motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-40",
  {
    variants: {
      variant: {
        default: "bg-[#ff9771] text-[#30313a] hover:bg-[#f67d57] active:bg-[#f67d57]",
        primary:
          "bg-gradient-to-b from-[hsl(var(--maple-primary))] to-[hsl(var(--maple-primary-strong))] text-[hsl(var(--maple-on-primary))]/90 hover:brightness-110",
        destructive:
          "bg-gradient-to-b from-[hsl(var(--maple-error))] to-[hsl(var(--maple-error)/0.8)] text-destructive-onFilled hover:brightness-110",
        outline:
          "border border-[#171717]/15 bg-transparent text-[#30313a] hover:bg-[#171717]/[0.04]",
        secondary:
          "bg-gradient-to-b from-[hsl(var(--maple-secondary-container))] to-[hsl(var(--maple-secondary-container)/0.6)] text-[hsl(var(--maple-secondary-700))] hover:brightness-110 dark:from-[hsl(var(--maple-secondary-container))] dark:to-[hsl(var(--maple-secondary-container)/0.4)] dark:text-[hsl(var(--maple-on-secondary))]",
        ghost:
          "text-foreground hover:bg-[hsl(var(--maple-secondary-container))] dark:hover:bg-[hsl(var(--maple-primary))]/15 dark:hover:text-foreground",
        link: "rounded-none text-[hsl(var(--maple-primary))] underline-offset-4 hover:underline"
      },
      size: {
        default: "min-h-12 px-6 py-3",
        sm: "h-9 rounded-md px-3",
        lg: "h-11 rounded-md px-8",
        icon: "h-10 w-10"
      }
    },
    defaultVariants: {
      variant: "default",
      size: "default"
    }
  }
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, onClick, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";

    return (
      <Comp
        className={cn(buttonVariants({ variant, size, className }))}
        ref={ref}
        onClick={onClick}
        {...props}
      />
    );
  }
);
Button.displayName = "Button";

// eslint-disable-next-line react-refresh/only-export-components
export { Button, buttonVariants };
