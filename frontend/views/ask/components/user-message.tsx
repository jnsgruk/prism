export const UserMessage = ({ content }: { content: string }): React.ReactElement => (
  <div className="flex justify-end">
    <div className="max-w-[85%] rounded-2xl rounded-tr-sm bg-primary px-4 py-2.5 text-primary-foreground">
      <p className="text-sm whitespace-pre-wrap">{content}</p>
    </div>
  </div>
);
