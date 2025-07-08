export interface TeamStatus {
  has_team_subscription: boolean;
  team_created: boolean;
  team_id?: string;
  team_name?: string;
  role?: "admin" | "member";
  seats_purchased?: number;
  seats_used?: number;
  seats_available?: number;
  members_count?: number;
  pending_invites_count?: number;
  created_at?: string;
  seat_limit_exceeded?: boolean;
  is_team_admin?: boolean;
}

export interface TeamMember {
  user_id: string;
  email: string;
  role: "admin" | "member";
  joined_at: string;
}

export interface TeamInvite {
  invite_id: string;
  email: string;
  expires_at: string;
  status: "pending" | "accepted" | "expired";
}

export interface TeamMembersResponse {
  members: TeamMember[];
  pending_invites: TeamInvite[];
}

export interface CreateTeamRequest {
  name: string;
}

export interface CreateTeamResponse {
  team_id: string;
  name: string;
  created_at: string;
}

export interface InviteMembersRequest {
  emails: string[];
}

export interface InviteMembersResponse {
  invites: TeamInvite[];
}

export interface CheckInviteResponse {
  valid: boolean;
  team_name?: string;
  invited_by_name?: string;
  expires_at?: string;
  status?: "pending" | "accepted" | "expired";
}

export interface AcceptInviteRequest {
  email: string;
}

export interface UpdateTeamNameResponse {
  team_id: string;
  name: string;
  updated_at: string;
}
