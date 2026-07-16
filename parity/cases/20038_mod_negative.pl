# Perl's % takes the sign of the RIGHT operand (C takes the left).
printf "%d\n", -7 % 3;
printf "%d\n", 7 % -3;
printf "%d\n", -7 % -3;
printf "%d\n", 7 % 3;
printf "%d\n", -1 % 5;
