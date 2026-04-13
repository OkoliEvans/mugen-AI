// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Ownable2Step, Ownable} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {Pausable}              from "@openzeppelin/contracts/utils/Pausable.sol";
import {ReentrancyGuard}       from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/// @title  VeilVault
/// @notice Pre-funded credit vault for Mugen Veil verifiable inference.
///
///         Users deposit HSK (native gas token of HashKey Chain) once.
///         The gateway deducts the proof fee per inference automatically —
///         no per-transaction wallet approval required.
///
///         Fee model:
///           STANDARD_FEE = 2 HSK per proof  (~$0.32 at $0.16/HSK)
///           PRIORITY_FEE = 5 HSK per proof  (faster batch flush)
///           Mugen margin: ~62% at current $PROVE prices
///
///         Lifecycle:
///           1. User calls deposit{value: N HSK}
///           2. Gateway calls deductFee(user, tier) after proof is submitted
///              to Succinct — not before, so failed proofs cost nothing
///           3. User can withdraw remaining balance at any time
///           4. Owner can update fees and the authorized gateway address
///
///         Security model:
///           - Only `gateway` address can call deductFee() — onlyGateway modifier
///           - Reentrancy guard on all state-changing calls
///           - Pausable for emergency stops
///           - Ownable2Step — two-transaction ownership transfer prevents accidents
///
/// @dev    Deployed on HashKey testnet (Chain ID 133).
///         HSK is the native token — no ERC-20 transfers needed.
///         All amounts in wei (1 HSK = 1e18 wei).
contract VeilVault is Ownable2Step, Pausable, ReentrancyGuard {

    // ── Constants ─────────────────────────────────────────────────────────────

    uint256 public constant STANDARD_FEE = 2 ether; // 2 HSK
    uint256 public constant PRIORITY_FEE = 5 ether; // 5 HSK

    // ── Storage ───────────────────────────────────────────────────────────────

    /// @notice HSK balance per user in wei.
    mapping(address => uint256) public balances;

    /// @notice Address authorized to call deductFee().
    ///         Set to the Mugen gateway address on deployment.
    address public gateway;

    /// @notice Total HSK collected as fees (not yet withdrawn by owner).
    uint256 public totalFeesCollected;

    /// @notice Total proofs paid for across all users.
    uint256 public totalProofsPaid;

    // ── Events ────────────────────────────────────────────────────────────────

    event Deposited(address indexed user, uint256 amount, uint256 newBalance);
    event Withdrawn(address indexed user, uint256 amount, uint256 remaining);
    event FeeDeducted(
        address indexed user,
        uint256 fee,
        ProofTier tier,
        uint256 remaining,
        string  jobId
    );
    event GatewayUpdated(address indexed oldGateway, address indexed newGateway);
    event FeesWithdrawn(address indexed to, uint256 amount);

    // ── Errors ────────────────────────────────────────────────────────────────

    error NotGateway(address caller);
    error InsufficientBalance(address user, uint256 required, uint256 available);
    error ZeroDeposit();
    error ZeroAddress();
    error ZeroWithdraw();
    error WithdrawFailed();
    error FeeWithdrawFailed();
    error NoFeesToWithdraw();

    // ── Types ─────────────────────────────────────────────────────────────────

    enum ProofTier {
        Standard, // 2 HSK — normal batch queue
        Priority  // 5 HSK — priority flush, faster settlement
    }

    // ── Modifiers ─────────────────────────────────────────────────────────────

    modifier onlyGateway() {
        if (msg.sender != gateway) revert NotGateway(msg.sender);
        _;
    }

    // ── Constructor ───────────────────────────────────────────────────────────

    /// @param _gateway      Address of the Mugen gateway (only caller of deductFee).
    /// @param _initialOwner Owner address (deployer).
    constructor(address _gateway, address _initialOwner) Ownable(_initialOwner) {
        if (_gateway      == address(0)) revert ZeroAddress();
        if (_initialOwner == address(0)) revert ZeroAddress();
        gateway = _gateway;
    }

    // ── User-facing ───────────────────────────────────────────────────────────

    /// @notice Deposit HSK into your Veil credit balance.
    ///         Send HSK as msg.value. No minimum — but you need at least
    ///         STANDARD_FEE (2 HSK) to submit a proof.
    ///
    /// @dev    At 2 HSK per proof, depositing 20 HSK gives you 10 proofs.
    ///         Top up whenever balance drops below your desired buffer.
    function deposit() external payable nonReentrant whenNotPaused {
        if (msg.value == 0) revert ZeroDeposit();
        balances[msg.sender] += msg.value;
        emit Deposited(msg.sender, msg.value, balances[msg.sender]);
    }

    /// @notice Withdraw any remaining HSK balance.
    ///         Callable at any time — funds are never locked.
    ///
    /// @param amount Amount in wei to withdraw. Must be <= current balance.
    function withdraw(uint256 amount) external nonReentrant whenNotPaused {
        if (amount == 0) revert ZeroWithdraw();
        uint256 available = balances[msg.sender];
        if (available < amount)
            revert InsufficientBalance(msg.sender, amount, available);

        balances[msg.sender] -= amount;

        (bool ok,) = msg.sender.call{value: amount}("");
        if (!ok) revert WithdrawFailed();

        emit Withdrawn(msg.sender, amount, balances[msg.sender]);
    }

    /// @notice Returns the HSK balance for a user in wei.
    function balanceOf(address user) external view returns (uint256) {
        return balances[user];
    }

    /// @notice Check whether a user has enough balance for a given proof tier.
    function canProve(address user, ProofTier tier) external view returns (bool) {
        return balances[user] >= _feeFor(tier);
    }

    // ── Gateway-facing ────────────────────────────────────────────────────────

    /// @notice Deduct the proof fee from a user's balance.
    ///         Called by the Mugen gateway after a proof job is successfully
    ///         submitted to the Succinct Network — not before, so failed
    ///         submissions cost the user nothing.
    ///
    /// @param user   Address of the user who submitted the job.
    /// @param tier   ProofTier.Standard (2 HSK) or ProofTier.Priority (5 HSK).
    /// @param jobId  Mugen job UUID — emitted in FeeDeducted for off-chain reconciliation.
    ///
    /// @dev  Reverts with InsufficientBalance if balance is too low.
    ///       The gateway should check canProve() before accepting the job
    ///       to fail fast without spending gas on the deduct call.
    function deductFee(
        address   user,
        ProofTier tier,
        string calldata jobId
    ) external nonReentrant whenNotPaused onlyGateway {
        uint256 fee      = _feeFor(tier);
        uint256 balance  = balances[user];

        if (balance < fee)
            revert InsufficientBalance(user, fee, balance);

        balances[user]       -= fee;
        totalFeesCollected   += fee;
        totalProofsPaid      += 1;

        emit FeeDeducted(user, fee, tier, balances[user], jobId);
    }

    // ── Admin ─────────────────────────────────────────────────────────────────

    /// @notice Update the authorized gateway address.
    ///         Use when deploying a new gateway version.
    function setGateway(address newGateway) external onlyOwner {
        if (newGateway == address(0)) revert ZeroAddress();
        address old = gateway;
        gateway = newGateway;
        emit GatewayUpdated(old, newGateway);
    }

    /// @notice Withdraw accumulated proof fees to a recipient address.
    ///         Only callable by owner. Does not touch user balances.
    function withdrawFees(address payable to) external onlyOwner nonReentrant {
        if (to == address(0)) revert ZeroAddress();
        uint256 amount = totalFeesCollected;
        if (amount == 0) revert NoFeesToWithdraw();

        totalFeesCollected = 0;

        (bool ok,) = to.call{value: amount}("");
        if (!ok) revert FeeWithdrawFailed();

        emit FeesWithdrawn(to, amount);
    }

    /// @notice Pause all deposits, withdrawals, and fee deductions.
    function pause()   external onlyOwner { _pause(); }

    /// @notice Resume normal operation.
    function unpause() external onlyOwner { _unpause(); }

    // ── Internal ──────────────────────────────────────────────────────────────

    function _feeFor(ProofTier tier) internal pure returns (uint256) {
        return tier == ProofTier.Priority ? PRIORITY_FEE : STANDARD_FEE;
    }

    // ── Fallback ──────────────────────────────────────────────────────────────

    /// @dev Accept plain HSK transfers as deposits.
    receive() external payable {
        if (msg.value == 0) revert ZeroDeposit();
        balances[msg.sender] += msg.value;
        emit Deposited(msg.sender, msg.value, balances[msg.sender]);
    }
}
