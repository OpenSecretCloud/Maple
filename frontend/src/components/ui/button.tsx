import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/utils/utils";

const buttonVariants = cva(
  "hover:backdrop-blur-xs inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium ring-offset-background transition-colors transition-opacity focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        default:
          "bg-primary backdrop-blur-xs text-primary-foreground hover:bg-primary/90 active:bg-primary/80",
        destructive: "bg-destructive text-white hover:bg-destructive/90",
        outline:
          "border border-[hsl(var(--purple))]/20 hover:border-[hsl(var(--purple))]/80 bg-background/80 hover:bg-background/80 hover:text-foreground dark:text-foreground dark:hover:text-white dark:hover:bg-[hsl(var(--purple))]/20 dark:border-[#3FDBFF]/20 dark:hover:border-[#3FDBFF]/80 dark:focus:text-white dark:active:text-white transition-all duration-300",
        secondary: "bg-secondary text-secondary-foreground hover:bg-secondary/80",
        ghost:
          "hover:bg-accent hover:text-accent-foreground dark:hover:bg-[hsl(var(--purple))]/20 dark:hover:text-white",
        link: "text-primary underline-offset-4 hover:underline"
      },
      size: {
        default: "h-10 px-4 py-2",
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
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, onClick = () => {}, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";

    // Memoize the onClick handler to prevent unnecessary re-renders
    const memoizedOnClick = React.useCallback(onClick, [onClick]);

    return (
      <Comp
        className={cn(buttonVariants({ variant, size, className }))}
        ref={ref}
        onClick={memoizedOnClick}
        {...props}
      />
    );
  }
);
Button.displayName = "Button";

// eslint-disable-next-line react-refresh/only-export-components
export { Button, buttonVariants };
