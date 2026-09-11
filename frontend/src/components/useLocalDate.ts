import { useEffect, useState } from "react";
import { localDateString } from "./DateRangeSelector";

function msUntilNextLocalMidnight(now: Date): number {
  const next = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  return next.getTime() - now.getTime();
}

/**
 * The current local calendar date as `YYYY-MM-DD`. It rolls over at local
 * midnight and re-checks whenever the page becomes visible or focused, since
 * timers are unreliable in background tabs and across sleep.
 */
export function useLocalDate(): string {
  const [today, setToday] = useState(() => localDateString(new Date()));

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const sync = () => {
      setToday(localDateString(new Date()));
      clearTimeout(timer);
      timer = setTimeout(sync, msUntilNextLocalMidnight(new Date()) + 1000);
    };
    timer = setTimeout(sync, msUntilNextLocalMidnight(new Date()) + 1000);
    document.addEventListener("visibilitychange", sync);
    window.addEventListener("focus", sync);
    return () => {
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", sync);
      window.removeEventListener("focus", sync);
    };
  }, []);

  return today;
}
