import { Breadcrumb, BreadcrumbItem, BreadcrumbList, BreadcrumbSeparator } from "@/components/ui/breadcrumb";
import { Command, CommandEmpty, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { useDebouncedValue } from "@/lib/hooks/use-debounced-value";
import { usePaginatedPeople } from "@/lib/hooks/use-org";
import { ChevronsUpDown } from "lucide-react";
import { useState } from "react";

export const PersonBreadcrumb = ({
  personName,
  personId,
  onSelect,
}: {
  personName: string;
  personId: string;
  onSelect: (personId: string) => void;
}): React.ReactElement => {
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const debouncedSearch = useDebouncedValue(search);

  const { data } = usePaginatedPeople({
    search: debouncedSearch || undefined,
    pageSize: 20,
    pageToken: "",
  });
  const people = data?.people ?? [];

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <button type="button" className="inline-flex items-center gap-1 rounded-md px-1.5 py-1 hover:bg-muted" />
        }
      >
        <Breadcrumb>
          <BreadcrumbList>
            <BreadcrumbItem className="text-sm font-medium text-foreground">People</BreadcrumbItem>
            <BreadcrumbSeparator />
            <BreadcrumbItem className="text-sm font-medium text-foreground">{personName}</BreadcrumbItem>
          </BreadcrumbList>
        </Breadcrumb>
        <ChevronsUpDown className="size-3 shrink-0 text-muted-foreground" />
      </PopoverTrigger>
      <PopoverContent className="w-80 p-0" align="start">
        <Command shouldFilter={false}>
          <CommandInput placeholder="Search people..." value={search} onValueChange={setSearch} />
          <CommandList>
            <CommandEmpty>No people found.</CommandEmpty>
            <CommandItem
              value="__all__"
              onSelect={() => {
                onSelect("__all__");
                setOpen(false);
                setSearch("");
              }}
            >
              <span className="text-muted-foreground">View all people</span>
            </CommandItem>
            {people.map((person) => (
              <CommandItem
                key={person.id}
                value={person.id}
                data-checked={person.id === personId ? "true" : undefined}
                onSelect={() => {
                  onSelect(person.id);
                  setOpen(false);
                  setSearch("");
                }}
              >
                <span className="flex min-w-0 flex-col">
                  <span className="truncate text-sm">{person.name}</span>
                  {person.email && <span className="truncate text-xs text-muted-foreground">{person.email}</span>}
                </span>
              </CommandItem>
            ))}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
};
