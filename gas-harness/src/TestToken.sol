// SPDX-License-Identifier: MIT
pragma solidity =0.8.28;

/// @notice The ONE token used by every algorithm in layer C.
///
/// Token transfer implementations are a first-order confounder in end-to-end gas: a token
/// with a fee-on-transfer hook, a rebasing balance, a blocklist check or a different storage
/// packing moves the number by hundreds of gas and has nothing to do with the curve. Every
/// pool in layer C therefore trades THIS token and only this token, and its source hash and
/// deployed-bytecode hash are recorded in the snapshot.
///
/// It is deliberately minimal and deliberately NOT optimised: two mappings, no permit, no
/// hooks, `unchecked` nowhere. `decimals()` is 18 because `ExamplePropAmm.createPair`
/// requires `decimalsX + xRetainDecimals == decimalsY + yRetainDecimals`.
contract TestToken {
    string public name;
    string public symbol;
    uint8 public constant decimals = 18;

    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);

    constructor(string memory name_, string memory symbol_) {
        name = name_;
        symbol = symbol_;
    }

    function mint(address to, uint256 amount) external {
        totalSupply += amount;
        balanceOf[to] += amount;
        emit Transfer(address(0), to, amount);
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        emit Approval(msg.sender, spender, amount);
        return true;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        _transfer(msg.sender, to, amount);
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        uint256 allowed = allowance[from][msg.sender];
        if (allowed != type(uint256).max) {
            require(allowed >= amount, "TestToken: allowance");
            allowance[from][msg.sender] = allowed - amount;
        }
        _transfer(from, to, amount);
        return true;
    }

    function _transfer(address from, address to, uint256 amount) private {
        uint256 bal = balanceOf[from];
        require(bal >= amount, "TestToken: balance");
        balanceOf[from] = bal - amount;
        balanceOf[to] += amount;
        emit Transfer(from, to, amount);
    }
}
