import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbSeparator,
} from "@/components/ui/breadcrumb";
import { Fragment } from "react";
import { Link } from "react-router";

import type { Team } from "@ps/api/gen/prism/v1/org_pb";

import { getAncestors } from "@/views/teams/hooks/use-teams";

export const TeamBreadcrumb = ({
  roots,
  selectedTeamId,
  selector,
}: {
  roots: Team[];
  selectedTeamId: string;
  /** Rendered as the last breadcrumb item (the team selector dropdown). */
  selector?: React.ReactNode;
}): React.ReactElement | null => {
  const ancestors = getAncestors(roots, selectedTeamId);
  // Ancestors minus the last (current team) — those become links
  const parentAncestors = ancestors.slice(0, -1);

  if (!selector && ancestors.length <= 1) return null;

  return (
    <Breadcrumb>
      <BreadcrumbList>
        {parentAncestors.map((team, i) => (
          <Fragment key={team.id}>
            {i > 0 && <BreadcrumbSeparator />}
            <BreadcrumbItem>
              <BreadcrumbLink render={<Link to={`/teams/${team.id}`} />}>
                {team.name}
              </BreadcrumbLink>
            </BreadcrumbItem>
          </Fragment>
        ))}
        {selector && (
          <>
            {parentAncestors.length > 0 && <BreadcrumbSeparator />}
            <BreadcrumbItem>{selector}</BreadcrumbItem>
          </>
        )}
      </BreadcrumbList>
    </Breadcrumb>
  );
};
