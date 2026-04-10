import { Button } from "@/components/ui/button";
import { FileQuestion } from "lucide-react";
import { Link } from "react-router-dom";

const NotFoundPage = (): React.ReactElement => (
  <div className="flex min-h-[60vh] items-center justify-center p-6">
    <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
      <FileQuestion className="mb-3 size-10 text-muted-foreground" />
      <p className="mb-1 font-medium">Page not found</p>
      <p className="mb-4 text-sm text-muted-foreground">The page you're looking for doesn't exist.</p>
      <Button nativeButton={false} render={<Link to="/" />}>
        Back to Dashboard
      </Button>
    </div>
  </div>
);

export default NotFoundPage;
