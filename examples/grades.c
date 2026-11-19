#include <stdio.h>

#define CLASS_SIZE 8
#define PASS_MARK 50

/* Sort the marks in place, ascending, with a simple bubble sort. */
void sort(int marks[], int n) {
    int i, j, tmp;
    for (i = 0; i < n - 1; i++) {
        for (j = 0; j < n - 1 - i; j++) {
            if (marks[j] > marks[j + 1]) {
                tmp = marks[j];
                marks[j] = marks[j + 1];
                marks[j + 1] = tmp;
            }
        }
    }
}

/* Sum every mark in the array. */
int total(int marks[], int n) {
    int i, sum = 0;
    for (i = 0; i < n; i++) {
        sum = sum + marks[i];
    }
    return sum;
}

/* Count how many marks reach the pass threshold. */
int passing(int marks[], int n) {
    int i, count = 0;
    for (i = 0; i < n; i++) {
        if (marks[i] >= PASS_MARK) {
            count = count + 1;
        }
    }
    return count;
}

int main(void) {
    int marks[CLASS_SIZE] = {72, 48, 91, 35, 66, 50, 80, 23};
    int i, sum, average, pass;

    sort(marks, CLASS_SIZE);

    sum = total(marks, CLASS_SIZE);
    average = sum / CLASS_SIZE;
    pass = passing(marks, CLASS_SIZE);

    printf("Sorted marks:");
    for (i = 0; i < CLASS_SIZE; i++) {
        printf(" %d", marks[i]);
    }
    printf("\n");

    printf("Lowest:  %d\n", marks[0]);
    printf("Highest: %d\n", marks[CLASS_SIZE - 1]);
    printf("Average: %d\n", average);
    printf("Passed:  %d of %d\n", pass, CLASS_SIZE);

    if (average >= PASS_MARK) {
        printf("Class result: PASS\n");
    } else {
        printf("Class result: FAIL\n");
    }

    return 0;
}
