# Core Engines

This directory is reserved for the core execution engines of Merchant OS:
- **Transaction Engine**: Coordinates atomic state changes and transactional integrity.
- **Business Engine**: Enforces business rules and validation invariants.

## Architecture Boundaries
1. All business-changing operations must route through the Transaction Engine and Business Engine before reaching persistence.
2. Direct unmediated writes to the storage layer are strictly prohibited.
