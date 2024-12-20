### More calculator examples

We can have multiple files to be tested


[//]: # (decisionTable Calculator.Fixtures.CalculatorFixture)

| a  | b   | sub? |
|----|-----|------|
| 1  | 2   | -1   |
| 2  | 2   | 0    |
| 10 | 5   | 5    |
| 10 | -20 | 30   |

We can also name our columns with a `set`

[//]: # (decisionTable Calculator.Fixtures.CalculatorFixture )

| setA | set B | div? |
|------|-------|------|
| 1    | 2     | 0    |
| 2    | 2     | 1    |
| 10   | 5     | 2    |
| 10   | -20   | 0    |

And we still need to implement exponential:

[//]: # (decisionTable Calculator.Fixtures.CalculatorFixture -- snooze until 2099-12-31 )

| a  | b   | exp? |
|----|-----|------|
| 1  | 2   | 1    |
| 2  | 2   | 4    |
| 3  | 2   | 9    |
| 3  | 3   | 27   |

Variables:

[//]: # (decisionTable Calculator.Fixtures.CalculatorFixture )

| a  | b | sum? |
|----|---|------|
| 1  | 2 | $V=  |
| $V | 2 | 5    |
| 2  | 1 | $V   |
