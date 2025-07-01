# Team Management Self-Service Implementation Requirements

## Overview
Implement self-service team management features allowing any authenticated user to purchase and manage teams without manual admin intervention. This is a critical B2B feature that will unlock millions in ARR.

## Key Changes from Previous Behavior

1. **Team Plan Visibility**: 
   - **REMOVE**: Any calls to `/subscription/team_plan_available` - this endpoint no longer exists
   - **NEW**: Show team plans to ALL authenticated users on the pricing page
   - No more whitelist checking needed

2. **Team Status Detection**:
   - The existing `/subscription/status` endpoint remains unchanged
   - Use the new `/team/status` endpoint to get team-specific information

## API Endpoints

### 1. Team Status
```
GET /team/status
Authorization: Bearer <jwt_token>

// Response when user has team plan but hasn't created team yet:
{
  "has_team_subscription": true,
  "team_created": false,
  "is_team_admin": true,
  "seats_purchased": 5
}

// Response after team creation (admin):
{
  "has_team_subscription": true,
  "team_created": true,
  "team_id": "uuid",
  "team_name": "My Awesome Team",
  "role": "admin",
  "seats_purchased": 5,
  "seats_used": 1,
  "seats_available": 4,
  "members_count": 1,
  "pending_invites_count": 0,
  "created_at": "2025-07-01T...",
  "seat_limit_exceeded": false
}
```

### 2. Create Team
```
POST /team/create
Authorization: Bearer <jwt_token>
Content-Type: application/json
{
  "name": "My Awesome Team"
}

// Success Response:
{
  "team_id": "uuid",
  "name": "My Awesome Team",
  "created_at": "2025-07-01T..."
}
```

### 3. Invite Members
```
POST /team/invites
Authorization: Bearer <jwt_token>
Content-Type: application/json
{
  "emails": ["john@example.com", "jane@example.com"]
}

// Response:
{
  "invites": [
    {
      "invite_id": "uuid",
      "email": "john@example.com",
      "expires_at": "2025-07-08T...",
      "status": "pending"
    },
    ...
  ]
}
```

### 4. List Members
```
GET /team/members
Authorization: Bearer <jwt_token>

// Response:
{
  "members": [
    {
      "user_id": "uuid",
      "email": "admin@example.com",
      "role": "admin",
      "joined_at": "2025-07-01T..."
    }
  ],
  "pending_invites": [
    {
      "invite_id": "uuid",
      "email": "john@example.com",
      "expires_at": "2025-07-08T...",
      "status": "pending"
    }
  ]
}
```

### 5. Check Invite
```
GET /team/invites/{invite_id}/check
Authorization: Bearer <jwt_token>

// Response:
{
  "valid": true,
  "team_name": "My Awesome Team",
  "invited_by_name": "Admin User",
  "expires_at": "2025-07-08T...",
  "status": "pending"
}
```

### 6. Accept Invite
```
POST /team/invites/{invite_id}/accept
Authorization: Bearer <jwt_token>
Content-Type: application/json
{
  "email": "john@example.com"  // User must confirm their email
}
```

### 7. Remove Member
```
DELETE /team/members/{user_id}
Authorization: Bearer <jwt_token>
```

### 8. Leave Team
```
POST /team/leave
Authorization: Bearer <jwt_token>
```

### 9. Revoke Invite
```
DELETE /team/invites/{invite_id}
Authorization: Bearer <jwt_token>
```

## Implementation Flow

1. **Pricing Page**: Show team plans to all authenticated users
2. **Account Menu Integration**: 
   - Add "Manage Team" option for users with team plan
   - Show alert badge if team plan purchased but team not created
   - All team features accessible from AccountMenu/AccountDialog
3. **Team Setup**: Simple form for team name when first accessing team management
4. **Team Dashboard**: Show after creation with member management
5. **Invite Flow**: Multi-email invite with seat checking
6. **Member Management**: List, remove, leave functionality
7. **Invite Acceptance**: Route handling with auth check

## Critical UI/UX Requirements

1. **Seat Counter**: Always show "X of Y seats used"
2. **Seat Limit Warning**: Red banner if seat_limit_exceeded
3. **Invite Management**: Show expiration countdown
4. **Error States**: Clear messages for all failure cases

## Error Handling

- Not enough seats: 400 Bad Request
- Already in a team: User can only be in one team
- Invite expired: Show appropriate message
- Team subscription cancelled: No new invites allowed

## Testing Checklist

- [ ] Purchase team plan as new user
- [ ] Create team with name
- [ ] Invite members up to seat limit
- [ ] Try to invite beyond seat limit (should fail)
- [ ] Accept invite from another account
- [ ] Remove a team member
- [ ] Member leaves team voluntarily
- [ ] Revoke a pending invite
- [ ] Check expired invite (after 7 days)
- [ ] Test seat limit exceeded state

## Implementation Status

- [x] Task 1: Remove team_plan_available usage
- [x] Task 2: Create team API functions
- [x] Task 3: Create type definitions
- [ ] Task 4: Update AccountMenu with team management integration
- [ ] Task 5: Create TeamSetupDialog
- [ ] Task 6: Create TeamDashboard component
- [ ] Task 7: Implement invite functionality
- [ ] Task 8: Create TeamMembersList
- [ ] Task 9: Implement remove member
- [ ] Task 10: Implement leave team
- [ ] Task 11: Create invite acceptance route
- [ ] Task 12: Create InvitePreview
- [ ] Task 13: Add seat limit warning
- [ ] Task 14: Implement revoke invite
- [ ] Task 15: Add error handling
- [ ] Task 16: Create TeamContext
- [ ] Task 17: Integrate team status
- [ ] Task 18: Write tests
- [ ] Task 19: Final review with git diff

## Codebase Investigation Summary

### Authentication & User Management
- Authentication via `@opensecret/react` SDK with JWT tokens
- User data accessible via `os.auth.user` from `useOpenSecret` hook
- Multiple auth providers: Email/Password, GitHub, Google, Apple
- Billing tokens stored separately in sessionStorage as `maple_billing_token`

### Billing & Subscription System
- Billing API URL: `import.meta.env.VITE_MAPLE_BILLING_API_URL`
- BillingService singleton with automatic token refresh
- Supports Stripe and Zaprite payment providers
- Current team plan detection via `fetchTeamPlanAvailable()` (TO BE REMOVED)
- Billing status tracked in LocalStateContext

### Routing Structure
- TanStack Router v1.50.1 with file-based routing
- Current routes: /, /pricing, /login, /signup, /chat/:chatId
- Need to add: /team/settings, /team/invite/:inviteId

### UI Components Available
- Radix UI based: Dialog, Alert, Badge, Card, Select, DropdownMenu
- Forms: Input, Label, Button, Textarea
- Dark mode support with CSS variables
- Tailwind CSS styling
- Missing: Table component for member lists

### State Management
- React Context API (no Redux)
- LocalStateContext manages user prompts, billing status, chat history
- TanStack Query for server state
- Need to create TeamContext for team state

### API Communication Patterns
- Fetch API with Bearer token auth
- Error handling with try/catch
- TypeScript response typing
- BillingService handles token refresh on 401

### Existing Team Code
- Team plan in pricing at $30/user
- Team plan visibility controlled by whitelist
- "Contact Us" button for non-whitelisted users
- Features listed: "Collaboration features", "Shared history", "Team administration"

### Key Implementation Files
```
/src/billing/billingApi.ts       - API endpoints
/src/billing/billingService.ts   - Service layer
/src/routes/pricing.tsx          - Pricing page
/src/components/AccountMenu.tsx  - User menu
/src/state/localStateContext.tsx - Global state
```

### New Files to Create
```
/src/types/team.ts              - Team type definitions
/src/team/teamApi.ts            - Team API endpoints
/src/state/TeamContext.tsx      - Team state management
/src/components/team/*          - Team UI components
/src/routes/team/settings.tsx   - Team dashboard route
/src/routes/team/invite.$inviteId.tsx - Invite route
```

### Integration Points
1. After Stripe purchase success → Check team status
2. AccountMenu → Add team settings link
3. Pricing page → Remove whitelist check
4. LocalStateContext → Add team status
5. Navigation → Add team routes