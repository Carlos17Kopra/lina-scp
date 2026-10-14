import { useEffect } from "react";
import { useToast } from "../components";
import { useNoticeStore } from "../stores/notices";

/** Shows notices pushed by stores as toasts and removes them from the queue. */
export function useNoticeToasts(): void {
  const notices = useNoticeStore((state) => state.notices);
  const toast = useToast();
  useEffect(() => {
    for (const notice of notices) {
      toast.show({ tone: notice.tone, message: notice.message, details: notice.details });
      useNoticeStore.getState().remove(notice.id);
    }
  }, [notices, toast]);
}
