export function formatResetDate(isoDateString: string | null | undefined): string {
  if (!isoDateString) return "Resets Monthly";

  try {
    const date = new Date(isoDateString);
    const now = new Date();

    // Check if date is valid
    if (isNaN(date.getTime())) {
      return "Resets Monthly";
    }

    // Calculate time difference in milliseconds
    const timeDiff = date.getTime() - now.getTime();

    // If reset date is in the past
    if (timeDiff < 0) {
      return "Resets Monthly";
    }

    // Get calendar dates for comparison
    const nowDate = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const resetDate = new Date(date.getFullYear(), date.getMonth(), date.getDate());

    // Calculate difference in calendar days
    const daysDiff = Math.round((resetDate.getTime() - nowDate.getTime()) / (1000 * 60 * 60 * 24));

    // If reset is today (same calendar day)
    if (daysDiff === 0) {
      const hoursUntil = Math.floor(timeDiff / (1000 * 60 * 60));
      if (hoursUntil < 1) {
        return "Resets in less than 1 hour";
      }
      if (hoursUntil === 1) {
        return "Resets in 1 hour";
      }
      return `Resets in ${hoursUntil} hours`;
    }

    // If reset is tomorrow (next calendar day)
    if (daysDiff === 1) {
      return "Resets Tomorrow";
    }

    // If reset is within 7 days
    if (daysDiff > 1 && daysDiff <= 7) {
      return `Resets in ${daysDiff} days`;
    }

    // For dates further in the future, show the actual date
    const options: Intl.DateTimeFormatOptions = {
      month: "short",
      day: "numeric"
    };

    return `Resets ${date.toLocaleDateString(undefined, options)}`;
  } catch (error) {
    console.error("Error formatting reset date:", error);
    return "Resets Monthly";
  }
}
