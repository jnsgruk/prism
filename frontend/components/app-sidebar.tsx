import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";
import {
  Activity,
  ChevronsUpDown,
  History,
  LogOut,
  MessageSquare,
  Settings,
  Sparkles,
  UserRound,
  Users,
} from "lucide-react";
import { Link, useLocation, useNavigate } from "react-router-dom";

import { PrismLogo } from "@/components/prism-logo";
import { useLogout } from "@ps/hooks/use-auth";
import { useListConversations } from "@/views/ask/hooks/use-conversations";

type User = {
  displayName: string;
  username: string;
};

const UserInitials = ({ name }: { name: string }): React.ReactElement => {
  const initials = name
    .split(" ")
    .map((n) => n[0])
    .join("")
    .toUpperCase()
    .slice(0, 2);

  return (
    <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-primary text-xs font-medium text-primary-foreground">
      {initials || "?"}
    </div>
  );
};

const UserInfoBlock = ({ user }: { user: User }): React.ReactElement => (
  <>
    <UserInitials name={user.displayName} />
    <div className="grid flex-1 text-left text-sm leading-tight">
      <span className="truncate font-semibold">{user.displayName}</span>
      <span className="truncate text-xs text-muted-foreground">{user.username}</span>
    </div>
  </>
);

const RecentChats = (): React.ReactElement => {
  const { data } = useListConversations(1, 5);
  const { pathname } = useLocation();
  const recent = data?.conversations.slice(0, 5) ?? [];

  return (
    <SidebarGroup>
      <SidebarGroupLabel>Recent chats</SidebarGroupLabel>
      <SidebarGroupContent>
        <SidebarMenu>
          {recent.map((conv) => (
            <SidebarMenuItem key={conv.id}>
              <SidebarMenuButton
                render={<Link to={`/ask/${conv.id}`} />}
                isActive={pathname === `/ask/${conv.id}`}
                tooltip={conv.title || "Untitled"}
              >
                <MessageSquare />
                <span>{conv.title || "Untitled"}</span>
              </SidebarMenuButton>
            </SidebarMenuItem>
          ))}
          <SidebarMenuItem>
            <SidebarMenuButton
              render={<Link to="/ask/history" />}
              isActive={pathname === "/ask/history"}
              tooltip="History"
            >
              <History />
              <span>History</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarGroupContent>
    </SidebarGroup>
  );
};

export const AppSidebar = ({ user }: { user: User }): React.ReactElement => {
  const { pathname } = useLocation();
  const navigate = useNavigate();
  const logout = useLogout();

  const isActive = (href: string): boolean => {
    if (href === "/") return pathname === "/";
    return pathname.startsWith(href);
  };

  return (
    <Sidebar collapsible="icon">
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton size="lg" render={<Link to="/" />} tooltip="Prism">
              <div className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-zinc-900 p-1.5 dark:bg-zinc-800">
                <PrismLogo size={24} />
              </div>
              <div className="grid flex-1 text-left leading-tight">
                <span className="truncate font-semibold">Prism</span>
                <span className="truncate text-xs text-muted-foreground">Engineering Insights</span>
              </div>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Insights</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton
                  render={<Link to="/ask" />}
                  isActive={pathname === "/ask"}
                  tooltip="Ask"
                >
                  <Sparkles />
                  <span>Ask</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>

        <RecentChats />

        <SidebarGroup>
          <SidebarGroupLabel>Organization</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton
                  render={<Link to="/teams" />}
                  isActive={isActive("/teams")}
                  tooltip="Teams"
                >
                  <Users />
                  <span>Teams</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
              <SidebarMenuItem>
                <SidebarMenuButton
                  render={<Link to="/people" />}
                  isActive={isActive("/people")}
                  tooltip="People"
                >
                  <UserRound />
                  <span>People</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>

        <SidebarGroup>
          <SidebarGroupLabel>Data</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton
                  render={<Link to="/ingestion" />}
                  isActive={isActive("/ingestion")}
                  tooltip="Ingestion"
                >
                  <Activity />
                  <span>Ingestion</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>

      <SidebarFooter>
        <SidebarMenu>
          <SidebarMenuItem>
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <SidebarMenuButton
                    size="lg"
                    className="data-open:bg-sidebar-accent data-open:text-sidebar-accent-foreground"
                  />
                }
              >
                <UserInfoBlock user={user} />
                <ChevronsUpDown className="ml-auto size-4" />
              </DropdownMenuTrigger>
              <DropdownMenuContent className="w-56" side="top" align="start" sideOffset={4}>
                <div className="flex items-center gap-2 px-2 py-1.5">
                  <UserInfoBlock user={user} />
                </div>
                <DropdownMenuSeparator />
                <DropdownMenuItem render={<Link to="/admin" />}>
                  <Settings className="mr-2 size-4" />
                  Admin
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  onClick={() => logout.mutate(undefined, { onSettled: () => navigate("/login") })}
                >
                  <LogOut className="mr-2 size-4" />
                  Log out
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  );
};
