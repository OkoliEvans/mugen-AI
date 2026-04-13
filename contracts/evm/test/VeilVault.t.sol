// SPDX-License-Identifier: MIT
// test/VeilVault.t.sol
pragma solidity ^0.8.20;

import {Test, console2} from "forge-std/Test.sol";
import {VeilVault} from "../src/VeilVault.sol";

contract VeilVaultTest is Test {
    // ── Fixtures ──────────────────────────────────────────────────────────────

    VeilVault internal vault;

    address internal owner = makeAddr("owner");
    address internal gateway = makeAddr("gateway");
    address internal alice = makeAddr("alice");
    address internal bob = makeAddr("bob");
    address internal carol = makeAddr("carol");

    uint256 internal constant STANDARD_FEE = 2 ether;
    uint256 internal constant PRIORITY_FEE = 5 ether;
    string internal constant JOB_ID = "550e8400-e29b-41d4-a716-446655440000";

    // ── Events (redeclared — Foundry requires events in scope to use expectEmit) ──

    event Deposited(address indexed user, uint256 amount, uint256 newBalance);
    event Withdrawn(address indexed user, uint256 amount, uint256 remaining);
    event FeeDeducted(
        address indexed user,
        uint256 fee,
        VeilVault.ProofTier tier,
        uint256 remaining,
        string jobId
    );
    event GatewayUpdated(
        address indexed oldGateway,
        address indexed newGateway
    );
    event FeesWithdrawn(address indexed to, uint256 amount);

    // ── Setup ─────────────────────────────────────────────────────────────────

    function setUp() public {
        vm.prank(owner);
        vault = new VeilVault(gateway, owner);

        // Fund test accounts with HSK
        vm.deal(alice, 100 ether);
        vm.deal(bob, 100 ether);
        vm.deal(carol, 100 ether);
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    function _deposit(address user, uint256 amount) internal {
        vm.prank(user);
        vault.deposit{value: amount}();
    }

    function _deduct(address user, VeilVault.ProofTier tier) internal {
        vm.prank(gateway);
        vault.deductFee(user, tier, JOB_ID);
    }

    // ── Constructor ───────────────────────────────────────────────────────────

    function test_constructor_setsGateway() public view {
        assertEq(vault.gateway(), gateway);
    }

    function test_constructor_setsOwner() public view {
        assertEq(vault.owner(), owner);
    }

    function test_constructor_revertZeroGateway() public {
        vm.expectRevert(VeilVault.ZeroAddress.selector);
        new VeilVault(address(0), owner);
    }

    function test_constructor_revertZeroOwner() public {
        vm.expectRevert(
            abi.encodeWithSelector(
                bytes4(keccak256("OwnableInvalidOwner(address)")),
                address(0)
            )
        );
        new VeilVault(gateway, address(0));
    }

    // ── Deposit ───────────────────────────────────────────────────────────────

    function test_deposit_updatesBalance() public {
        _deposit(alice, 10 ether);
        assertEq(vault.balanceOf(alice), 10 ether);
    }

    function test_deposit_multipleDeposits() public {
        _deposit(alice, 5 ether);
        _deposit(alice, 3 ether);
        assertEq(vault.balanceOf(alice), 8 ether);
    }

    function test_deposit_emitsEvent() public {
        vm.expectEmit(true, false, false, true);
        emit Deposited(alice, 10 ether, 10 ether);
        _deposit(alice, 10 ether);
    }

    function test_deposit_revertZeroValue() public {
        vm.prank(alice);
        vm.expectRevert(VeilVault.ZeroDeposit.selector);
        vault.deposit{value: 0}();
    }

    function test_deposit_isolatedPerUser() public {
        _deposit(alice, 10 ether);
        _deposit(bob, 20 ether);
        assertEq(vault.balanceOf(alice), 10 ether);
        assertEq(vault.balanceOf(bob), 20 ether);
    }

    function testFuzz_deposit(uint256 amount) public {
        amount = bound(amount, 1, 100 ether);
        vm.deal(alice, amount);
        _deposit(alice, amount);
        assertEq(vault.balanceOf(alice), amount);
    }

    // ── Receive (plain transfer) ──────────────────────────────────────────────

    function test_receive_creditsBalance() public {
        vm.prank(alice);
        (bool ok, ) = address(vault).call{value: 5 ether}("");
        assertTrue(ok);
        assertEq(vault.balanceOf(alice), 5 ether);
    }

    function test_receive_emitsDeposited() public {
        vm.expectEmit(true, false, false, true);
        emit Deposited(alice, 5 ether, 5 ether);
        vm.prank(alice);
        (bool ok, ) = address(vault).call{value: 5 ether}("");
        assertTrue(ok);
    }

    function test_receive_revertZeroValue() public {
        vm.prank(alice);
        vm.expectRevert(VeilVault.ZeroDeposit.selector);
        (bool ok, ) = address(vault).call{value: 0}("");
        // ok is always false when expectRevert is used — no assertion needed
        (ok); // silence unused variable warning
    }

    // ── Withdraw ──────────────────────────────────────────────────────────────

    function test_withdraw_reducesBalance() public {
        _deposit(alice, 10 ether);
        vm.prank(alice);
        vault.withdraw(6 ether);
        assertEq(vault.balanceOf(alice), 4 ether);
    }

    function test_withdraw_transfersHSK() public {
        _deposit(alice, 10 ether);
        uint256 before = alice.balance;
        vm.prank(alice);
        vault.withdraw(6 ether);
        assertEq(alice.balance, before + 6 ether);
    }

    function test_withdraw_fullBalance() public {
        _deposit(alice, 10 ether);
        vm.prank(alice);
        vault.withdraw(10 ether);
        assertEq(vault.balanceOf(alice), 0);
    }

    function test_withdraw_emitsEvent() public {
        _deposit(alice, 10 ether);
        vm.expectEmit(true, false, false, true);
        emit Withdrawn(alice, 6 ether, 4 ether);
        vm.prank(alice);
        vault.withdraw(6 ether);
    }

    function test_withdraw_revertZeroAmount() public {
        _deposit(alice, 10 ether);
        vm.prank(alice);
        vm.expectRevert(VeilVault.ZeroWithdraw.selector);
        vault.withdraw(0);
    }

    function test_withdraw_revertInsufficientBalance() public {
        _deposit(alice, 5 ether);
        vm.prank(alice);
        vm.expectRevert(
            abi.encodeWithSelector(
                VeilVault.InsufficientBalance.selector,
                alice,
                6 ether,
                5 ether
            )
        );
        vault.withdraw(6 ether);
    }

    function test_withdraw_revertNoBalance() public {
        vm.prank(alice);
        vm.expectRevert(
            abi.encodeWithSelector(
                VeilVault.InsufficientBalance.selector,
                alice,
                1 ether,
                0
            )
        );
        vault.withdraw(1 ether);
    }

    function testFuzz_withdraw(uint256 deposit, uint256 amount) public {
        deposit = bound(deposit, 1, 100 ether);
        amount = bound(amount, 1, deposit);
        vm.deal(alice, deposit);
        _deposit(alice, deposit);
        vm.prank(alice);
        vault.withdraw(amount);
        assertEq(vault.balanceOf(alice), deposit - amount);
    }

    // ── deductFee — Standard ──────────────────────────────────────────────────

    function test_deductFee_standard_reducesBalance() public {
        _deposit(alice, 10 ether);
        _deduct(alice, VeilVault.ProofTier.Standard);
        assertEq(vault.balanceOf(alice), 10 ether - STANDARD_FEE);
    }

    function test_deductFee_standard_updatesTotals() public {
        _deposit(alice, 10 ether);
        _deduct(alice, VeilVault.ProofTier.Standard);
        assertEq(vault.totalFeesCollected(), STANDARD_FEE);
        assertEq(vault.totalProofsPaid(), 1);
    }

    function test_deductFee_standard_emitsEvent() public {
        _deposit(alice, 10 ether);
        vm.expectEmit(true, false, false, true);
        emit FeeDeducted(
            alice,
            STANDARD_FEE,
            VeilVault.ProofTier.Standard,
            10 ether - STANDARD_FEE,
            JOB_ID
        );
        _deduct(alice, VeilVault.ProofTier.Standard);
    }

    // ── deductFee — Priority ──────────────────────────────────────────────────

    function test_deductFee_priority_reducesBalance() public {
        _deposit(alice, 10 ether);
        _deduct(alice, VeilVault.ProofTier.Priority);
        assertEq(vault.balanceOf(alice), 10 ether - PRIORITY_FEE);
    }

    function test_deductFee_priority_updatesTotals() public {
        _deposit(alice, 10 ether);
        _deduct(alice, VeilVault.ProofTier.Priority);
        assertEq(vault.totalFeesCollected(), PRIORITY_FEE);
        assertEq(vault.totalProofsPaid(), 1);
    }

    // ── deductFee — accumulation ──────────────────────────────────────────────

    function test_deductFee_multipleJobs_accumulatesTotals() public {
        _deposit(alice, 20 ether);
        _deduct(alice, VeilVault.ProofTier.Standard);
        _deduct(alice, VeilVault.ProofTier.Standard);
        _deduct(alice, VeilVault.ProofTier.Priority);
        assertEq(vault.totalFeesCollected(), STANDARD_FEE * 2 + PRIORITY_FEE);
        assertEq(vault.totalProofsPaid(), 3);
    }

    function test_deductFee_multipleUsers_isolatedBalances() public {
        _deposit(alice, 10 ether);
        _deposit(bob, 10 ether);
        _deduct(alice, VeilVault.ProofTier.Standard);
        _deduct(bob, VeilVault.ProofTier.Priority);
        assertEq(vault.balanceOf(alice), 10 ether - STANDARD_FEE);
        assertEq(vault.balanceOf(bob), 10 ether - PRIORITY_FEE);
        assertEq(vault.totalFeesCollected(), STANDARD_FEE + PRIORITY_FEE);
        assertEq(vault.totalProofsPaid(), 2);
    }

    // ── deductFee — access control ────────────────────────────────────────────

    function test_deductFee_revertNotGateway() public {
        _deposit(alice, 10 ether);
        vm.prank(alice);
        vm.expectRevert(
            abi.encodeWithSelector(VeilVault.NotGateway.selector, alice)
        );
        vault.deductFee(alice, VeilVault.ProofTier.Standard, JOB_ID);
    }

    function test_deductFee_revertOwnerNotGateway() public {
        _deposit(alice, 10 ether);
        vm.prank(owner);
        vm.expectRevert(
            abi.encodeWithSelector(VeilVault.NotGateway.selector, owner)
        );
        vault.deductFee(alice, VeilVault.ProofTier.Standard, JOB_ID);
    }

    // ── deductFee — insufficient balance ──────────────────────────────────────

    function test_deductFee_revertInsufficientBalance_standard() public {
        _deposit(alice, 1 ether);
        vm.prank(gateway);
        vm.expectRevert(
            abi.encodeWithSelector(
                VeilVault.InsufficientBalance.selector,
                alice,
                STANDARD_FEE,
                1 ether
            )
        );
        vault.deductFee(alice, VeilVault.ProofTier.Standard, JOB_ID);
    }

    function test_deductFee_revertInsufficientBalance_priority() public {
        _deposit(alice, 3 ether);
        vm.prank(gateway);
        vm.expectRevert(
            abi.encodeWithSelector(
                VeilVault.InsufficientBalance.selector,
                alice,
                PRIORITY_FEE,
                3 ether
            )
        );
        vault.deductFee(alice, VeilVault.ProofTier.Priority, JOB_ID);
    }

    function test_deductFee_revertZeroBalance() public {
        vm.prank(gateway);
        vm.expectRevert(
            abi.encodeWithSelector(
                VeilVault.InsufficientBalance.selector,
                alice,
                STANDARD_FEE,
                0
            )
        );
        vault.deductFee(alice, VeilVault.ProofTier.Standard, JOB_ID);
    }

    // ── canProve ──────────────────────────────────────────────────────────────

    function test_canProve_trueWhenSufficientBalance() public {
        _deposit(alice, STANDARD_FEE);
        assertTrue(vault.canProve(alice, VeilVault.ProofTier.Standard));
    }

    function test_canProve_falseWhenInsufficientBalance() public {
        _deposit(alice, 1 ether);
        assertFalse(vault.canProve(alice, VeilVault.ProofTier.Standard));
    }

    function test_canProve_priority_trueWhenSufficientBalance() public {
        _deposit(alice, PRIORITY_FEE);
        assertTrue(vault.canProve(alice, VeilVault.ProofTier.Priority));
    }

    function test_canProve_priority_falseWhenOnlyStandardBalance() public {
        _deposit(alice, STANDARD_FEE);
        assertFalse(vault.canProve(alice, VeilVault.ProofTier.Priority));
    }

    function test_canProve_falseAfterDeduction() public {
        _deposit(alice, STANDARD_FEE);
        _deduct(alice, VeilVault.ProofTier.Standard);
        assertFalse(vault.canProve(alice, VeilVault.ProofTier.Standard));
    }

    // ── setGateway ────────────────────────────────────────────────────────────

    function test_setGateway_updatesGateway() public {
        address newGateway = makeAddr("newGateway");
        vm.prank(owner);
        vault.setGateway(newGateway);
        assertEq(vault.gateway(), newGateway);
    }

    function test_setGateway_emitsEvent() public {
        address newGateway = makeAddr("newGateway");
        vm.expectEmit(true, true, false, false);
        emit GatewayUpdated(gateway, newGateway);
        vm.prank(owner);
        vault.setGateway(newGateway);
    }

    function test_setGateway_newGatewayCanDeduct() public {
        address newGateway = makeAddr("newGateway");
        vm.prank(owner);
        vault.setGateway(newGateway);

        _deposit(alice, 10 ether);
        vm.prank(newGateway);
        vault.deductFee(alice, VeilVault.ProofTier.Standard, JOB_ID);
        assertEq(vault.balanceOf(alice), 10 ether - STANDARD_FEE);
    }

    function test_setGateway_oldGatewayCanNoLongerDeduct() public {
        address newGateway = makeAddr("newGateway");
        vm.prank(owner);
        vault.setGateway(newGateway);

        _deposit(alice, 10 ether);
        vm.prank(gateway);
        vm.expectRevert(
            abi.encodeWithSelector(VeilVault.NotGateway.selector, gateway)
        );
        vault.deductFee(alice, VeilVault.ProofTier.Standard, JOB_ID);
    }

    function test_setGateway_revertZeroAddress() public {
        vm.prank(owner);
        vm.expectRevert(VeilVault.ZeroAddress.selector);
        vault.setGateway(address(0));
    }

    function test_setGateway_revertNotOwner() public {
        vm.prank(alice);
        vm.expectRevert();
        vault.setGateway(makeAddr("x"));
    }

    // ── withdrawFees ─────────────────────────────────────────────────────────

    function test_withdrawFees_transfersAccumulatedFees() public {
        _deposit(alice, 10 ether);
        _deduct(alice, VeilVault.ProofTier.Standard);
        _deduct(alice, VeilVault.ProofTier.Priority);

        address payable recipient = payable(makeAddr("recipient"));
        uint256 expected = STANDARD_FEE + PRIORITY_FEE;

        vm.prank(owner);
        vault.withdrawFees(recipient);

        assertEq(recipient.balance, expected);
        assertEq(vault.totalFeesCollected(), 0);
    }

    function test_withdrawFees_emitsEvent() public {
        _deposit(alice, 10 ether);
        _deduct(alice, VeilVault.ProofTier.Standard);

        address payable recipient = payable(makeAddr("recipient"));
        vm.expectEmit(true, false, false, true);
        emit FeesWithdrawn(recipient, STANDARD_FEE);

        vm.prank(owner);
        vault.withdrawFees(recipient);
    }

    function test_withdrawFees_doesNotTouchUserBalances() public {
        _deposit(alice, 10 ether);
        _deduct(alice, VeilVault.ProofTier.Standard);

        uint256 aliceBalanceBefore = vault.balanceOf(alice);

        address payable recipient = payable(makeAddr("recipient"));
        vm.prank(owner);
        vault.withdrawFees(recipient);

        assertEq(vault.balanceOf(alice), aliceBalanceBefore);
    }

    function test_withdrawFees_revertNoFees() public {
        address payable recipient = payable(makeAddr("recipient"));
        vm.prank(owner);
        vm.expectRevert(VeilVault.NoFeesToWithdraw.selector);
        vault.withdrawFees(recipient);
    }

    function test_withdrawFees_revertZeroAddress() public {
        _deposit(alice, 10 ether);
        _deduct(alice, VeilVault.ProofTier.Standard);
        vm.prank(owner);
        vm.expectRevert(VeilVault.ZeroAddress.selector);
        vault.withdrawFees(payable(address(0)));
    }

    function test_withdrawFees_revertNotOwner() public {
        _deposit(alice, 10 ether);
        _deduct(alice, VeilVault.ProofTier.Standard);
        vm.prank(alice);
        vm.expectRevert();
        vault.withdrawFees(payable(alice));
    }

    // ── Pause ─────────────────────────────────────────────────────────────────

    function test_pause_blocksDeposit() public {
        vm.prank(owner);
        vault.pause();
        vm.prank(alice);
        vm.expectRevert();
        vault.deposit{value: 1 ether}();
    }

    function test_pause_blocksWithdraw() public {
        _deposit(alice, 10 ether);
        vm.prank(owner);
        vault.pause();
        vm.prank(alice);
        vm.expectRevert();
        vault.withdraw(1 ether);
    }

    function test_pause_blocksDeductFee() public {
        _deposit(alice, 10 ether);
        vm.prank(owner);
        vault.pause();
        vm.prank(gateway);
        vm.expectRevert();
        vault.deductFee(alice, VeilVault.ProofTier.Standard, JOB_ID);
    }

    function test_unpause_restoresDeposit() public {
        vm.prank(owner);
        vault.pause();
        vm.prank(owner);
        vault.unpause();
        _deposit(alice, 5 ether);
        assertEq(vault.balanceOf(alice), 5 ether);
    }

    function test_pause_revertNotOwner() public {
        vm.prank(alice);
        vm.expectRevert();
        vault.pause();
    }

    function test_unpause_revertNotOwner() public {
        vm.prank(owner);
        vault.pause();
        vm.prank(alice);
        vm.expectRevert();
        vault.unpause();
    }

    // ── Ownership (Ownable2Step) ──────────────────────────────────────────────

    function test_ownership_transferIsTwoStep() public {
        address newOwner = makeAddr("newOwner");

        vm.prank(owner);
        vault.transferOwnership(newOwner);

        // Pending — not yet transferred
        assertEq(vault.owner(), owner);
        assertEq(vault.pendingOwner(), newOwner);

        // New owner must accept
        vm.prank(newOwner);
        vault.acceptOwnership();

        assertEq(vault.owner(), newOwner);
    }

    function test_ownership_revertAcceptByWrongAddress() public {
        address newOwner = makeAddr("newOwner");

        vm.prank(owner);
        vault.transferOwnership(newOwner);

        vm.prank(alice);
        vm.expectRevert();
        vault.acceptOwnership();
    }

    // ── Full lifecycle ────────────────────────────────────────────────────────

    function test_fullLifecycle_depositProveWithdraw() public {
        // Alice deposits 10 HSK
        _deposit(alice, 10 ether);
        assertEq(vault.balanceOf(alice), 10 ether);
        assertTrue(vault.canProve(alice, VeilVault.ProofTier.Standard));

        // Gateway deducts 3 standard proofs
        _deduct(alice, VeilVault.ProofTier.Standard);
        _deduct(alice, VeilVault.ProofTier.Standard);
        _deduct(alice, VeilVault.ProofTier.Standard);
        assertEq(vault.balanceOf(alice), 10 ether - STANDARD_FEE * 3);
        assertEq(vault.totalProofsPaid(), 3);
        assertEq(vault.totalFeesCollected(), STANDARD_FEE * 3);

        // Alice tops up
        _deposit(alice, 5 ether);

        // One priority proof
        _deduct(alice, VeilVault.ProofTier.Priority);
        assertEq(vault.totalProofsPaid(), 4);

        // Alice withdraws remaining balance
        uint256 remaining = vault.balanceOf(alice);
        uint256 aliceBefore = alice.balance;
        vm.prank(alice);
        vault.withdraw(remaining);
        assertEq(alice.balance, aliceBefore + remaining);
        assertEq(vault.balanceOf(alice), 0);

        // Owner withdraws fees
        address payable treasury = payable(makeAddr("treasury"));
        uint256 expectedFees = STANDARD_FEE * 3 + PRIORITY_FEE;
        vm.prank(owner);
        vault.withdrawFees(treasury);
        assertEq(treasury.balance, expectedFees);
        assertEq(vault.totalFeesCollected(), 0);
        // totalProofsPaid is not reset — it's a lifetime counter
        assertEq(vault.totalProofsPaid(), 4);
    }
}
